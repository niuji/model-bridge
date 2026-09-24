use super::{
    backup,
    journal::{self, Job, Phase},
    progress::DownloadProgress,
    release, Paths,
};
use anyhow::{ensure, Context, Result};
use std::{path::Path, time::Duration};

pub async fn systemctl(args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("systemctl");
    command.arg("--user").args(args).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(180), command.output())
        .await
        .context("systemctl timed out")??;
    ensure!(
        output.status.success(),
        "systemctl {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

pub fn extract(archive: &Path, destination: &Path) -> Result<()> {
    use std::io::Read;
    let file = std::fs::File::open(archive)?;
    let gzip = flate2::read::GzDecoder::new(file).take(release::MAX_BYTES + 1);
    let mut tar = tar::Archive::new(gzip);
    let mut found = false;
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            entry.header().entry_type().is_file(),
            "archive contains non-regular entry"
        );
        ensure!(
            path == Path::new("model-bridge") || path == Path::new("model-bridge.toml.example"),
            "unexpected archive path"
        );
        ensure!(
            entry.size() <= release::MAX_BYTES,
            "unpacked file too large"
        );
        if path == Path::new("model-bridge") {
            ensure!(!found, "duplicate binary in archive");
            found = true;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o700);
            }
            let mut out = options.open(destination)?;
            std::io::copy(&mut entry, &mut out)?;
            out.sync_all()?;
        } else {
            std::io::copy(&mut entry, &mut std::io::sink())?;
        }
    }
    let mut gzip = tar.into_inner();
    std::io::copy(&mut gzip, &mut std::io::sink())?;
    ensure!(gzip.limit() > 0, "unpacked archive exceeds size limit");
    ensure!(found, "archive has no binary");
    let mut header = [0u8; 20];
    std::fs::File::open(destination)?.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"\x7fELF"
            && header[4] == 2
            && header[5] == 1
            && header[18] == 62
            && header[19] == 0,
        "expected Linux x86_64 ELF binary"
    );
    Ok(())
}
async fn verify_version(path: &Path, version: &str) -> Result<()> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(path)
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == format!("model-bridge {version}"),
        "downloaded binary version mismatch"
    );
    Ok(())
}

trait Service {
    async fn command(&self, args: &[&str]) -> Result<String>;
    async fn ready(&self, paths: &Paths, job: &Job, version: &str, active: bool) -> Result<()>;
}
struct Systemd;
impl Service for Systemd {
    async fn command(&self, args: &[&str]) -> Result<String> {
        systemctl(args).await
    }
    async fn ready(&self, paths: &Paths, job: &Job, version: &str, active: bool) -> Result<()> {
        await_ready(paths, job, version, active).await
    }
}
pub async fn run(paths: Paths) -> Result<()> {
    let _lock = paths.lock()?;
    let mut job = paths.job()?.context("no update job")?;
    if job.terminal() {
        return Ok(());
    }
    let result = if job.phase == Phase::Downloading {
        install(&paths, &mut job, &Systemd).await
    } else {
        recover(&paths, &mut job, &Systemd).await
    };
    if let Err(error) = result {
        job.error = Some(format!("{error:#}"));
        paths.save(&job)?;
        if let Err(recovery) = recover(&paths, &mut job, &Systemd).await {
            job.error = Some(format!("{error:#}; recovery: {recovery:#}"));
            job.phase = Phase::RecoveryRequired;
            paths.save(&job)?;
            anyhow::bail!("{}", job.error.as_deref().unwrap_or("update failed"));
        }
    }
    if job.terminal() {
        if let Err(e) = record_result(&paths, &job) {
            tracing::warn!("update backup cleanup failed: {e}");
        }
    }
    Ok(())
}
async fn install(paths: &Paths, job: &mut Job, service: &impl Service) -> Result<()> {
    let dir = paths.job_dir(job)?;
    let expected: release::ReleaseInfo =
        journal::read_json(&dir.join("release.json"))?.context("missing pinned release")?;
    let client = release::client()?;
    let current = release::pinned_release(&client, &job.version).await?;
    ensure!(
        current.sha256 == expected.sha256
            && current.size == expected.size
            && current.asset_url == expected.asset_url,
        "release changed since version check"
    );
    ensure!(
        backup::digest(&paths.install.binary)? == job.old_sha256,
        "installed binary changed during update"
    );
    let archive = dir.join(format!("download-{}.tar.gz", uuid::Uuid::new_v4()));
    let mut last_progress = None::<std::time::Instant>;
    release::download(&client, &current, &archive, |downloaded_bytes| {
        if downloaded_bytes == 0
            || downloaded_bytes == current.size
            || last_progress.is_none_or(|last| last.elapsed() >= Duration::from_millis(500))
        {
            last_progress = Some(std::time::Instant::now());
            let progress = DownloadProgress {
                downloaded_bytes,
                total_bytes: current.size,
            };
            // Progress is advisory; an unavailable progress file must not abort installation.
            if let Err(error) = journal::write_json(&dir.join("progress.json"), &progress) {
                tracing::warn!("cannot save download progress: {error}");
            }
        }
    })
    .await?;
    let candidate = dir.join(format!("candidate-{}", uuid::Uuid::new_v4()));
    extract(&archive, &candidate)?;
    verify_version(&candidate, &job.version).await?;
    // Stage beside the executable before stopping service: rename will not cross filesystems.
    let staged = paths
        .install
        .binary
        .with_file_name(".model-bridge-update-staged");
    backup::atomic_copy(&candidate, &staged, true)?;
    let required = std::fs::metadata(&paths.database)?
        .len()
        .saturating_mul(3)
        .saturating_add(release::MAX_BYTES);
    ensure!(
        fs2::available_space(&paths.root)? > required,
        "insufficient space for database backup"
    );
    activate(paths, job, &staged, service).await
}
async fn activate(
    paths: &Paths,
    job: &mut Job,
    staged: &Path,
    service: &impl Service,
) -> Result<()> {
    let dir = paths.job_dir(job)?;
    job.phase = Phase::Stopping;
    paths.save(job)?;
    service.command(&["stop", "model-bridge.service"]).await?;
    stopped(service).await?;
    let drained: Option<String> = journal::read_json(&dir.join("drained.json"))?;
    ensure!(
        drained.as_deref() == Some(&job.id),
        "requests did not drain cleanly; update cancelled"
    );
    backup::snapshot(&paths.database, &dir.join("database.backup")).await?;
    backup::atomic_copy(&paths.install.binary, &dir.join("binary.backup"), true)?;
    job.database_sha256 = Some(backup::digest(&dir.join("database.backup"))?);
    job.backup_complete = true;
    job.phase = Phase::BackedUp;
    paths.save(job)?;
    std::fs::rename(staged, &paths.install.binary)?;
    journal::sync_dir(
        paths
            .install
            .binary
            .parent()
            .context("binary parent missing")?,
    )?;
    job.phase = Phase::Validating;
    paths.save(job)?;
    service
        .command(&["reset-failed", "model-bridge.service"])
        .await?;
    service.command(&["start", "model-bridge.service"]).await?;
    service.ready(paths, job, &job.version, false).await?;
    job.committed = true;
    job.phase = Phase::Committed;
    paths.save(job)?;
    service.ready(paths, job, &job.version, true).await?;
    job.phase = Phase::Succeeded;
    job.error = None;
    paths.save(job)?;
    Ok(())
}
async fn stopped(service: &impl Service) -> Result<()> {
    let pid = service
        .command(&[
            "show",
            "model-bridge.service",
            "--property=MainPID",
            "--value",
        ])
        .await?;
    ensure!(pid.trim() == "0", "main service is still running");
    Ok(())
}

async fn recover(paths: &Paths, job: &mut Job, service: &impl Service) -> Result<()> {
    if job.committed {
        // Once traffic was permitted, restoring the snapshot would discard new business writes.
        service
            .command(&["reset-failed", "model-bridge.service"])
            .await?;
        service.command(&["start", "model-bridge.service"]).await?;
        service.ready(paths, job, &job.version, true).await?;
        job.phase = Phase::Succeeded;
        paths.save(job)?;
        return Ok(());
    }
    if job.rollback_committed {
        service
            .command(&["reset-failed", "model-bridge.service"])
            .await?;
        service.command(&["start", "model-bridge.service"]).await?;
        service.ready(paths, job, &job.old_version, true).await?;
        job.phase = Phase::RolledBack;
        paths.save(job)?;
        return Ok(());
    }
    if !job.backup_complete {
        ensure!(
            backup::digest(&paths.install.binary)? == job.old_sha256,
            "original binary changed; recovery needs inspection"
        );
        // No migrations have run yet; original database is still authoritative.
        job.phase = Phase::RestartingOld;
        paths.save(job)?;
        service
            .command(&["reset-failed", "model-bridge.service"])
            .await?;
        service.command(&["start", "model-bridge.service"]).await?;
        service.ready(paths, job, &job.old_version, true).await?;
        job.phase = Phase::Failed;
        paths.save(job)?;
        return Ok(());
    }
    let dir = paths.job_dir(job)?;
    ensure!(
        backup::digest(&dir.join("binary.backup"))? == job.old_sha256,
        "old binary backup corrupt"
    );
    ensure!(
        Some(backup::digest(&dir.join("database.backup"))?) == job.database_sha256,
        "database backup corrupt"
    );
    job.phase = Phase::RollingBack;
    paths.save(job)?;
    service.command(&["stop", "model-bridge.service"]).await?;
    stopped(service).await?;
    restore_database(paths, job)?;
    backup::atomic_copy(&dir.join("binary.backup"), &paths.install.binary, true)?;
    job.phase = Phase::RollbackValidating;
    paths.save(job)?;
    service
        .command(&["reset-failed", "model-bridge.service"])
        .await?;
    service.command(&["start", "model-bridge.service"]).await?;
    service.ready(paths, job, &job.old_version, false).await?;
    job.rollback_committed = true;
    paths.save(job)?;
    service.ready(paths, job, &job.old_version, true).await?;
    job.phase = Phase::RolledBack;
    paths.save(job)?;
    Ok(())
}
fn restore_database(paths: &Paths, job: &Job) -> Result<()> {
    ensure!(
        !job.committed && !job.rollback_committed,
        "cannot restore database after traffic commit"
    );
    let dir = paths.job_dir(job)?;
    let quarantine = paths
        .database
        .with_file_name(format!("model-bridge-failed-{}", uuid::Uuid::new_v4()));
    journal::private_dir(&quarantine)?;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut source = paths.database.as_os_str().to_os_string();
        source.push(suffix);
        let source = std::path::PathBuf::from(source);
        if source.exists() {
            std::fs::rename(&source, quarantine.join(format!("database{suffix}")))?;
        }
    }
    journal::sync_dir(&quarantine)?;
    backup::atomic_copy(&dir.join("database.backup"), &paths.database, false)?;
    Ok(())
}
async fn await_ready(paths: &Paths, job: &Job, version: &str, active: bool) -> Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()?;
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if let Ok(response) = client
                .get(format!("{}/api/admin/update/readiness", paths.admin_url))
                .send()
                .await
            {
                if response.status().is_success() {
                    if let Ok(body) = response.json::<serde_json::Value>().await {
                        if body["version"] == version
                            && body["job_id"] == job.id
                            && body["ready"] == true
                            && body["active"] == active
                        {
                            return;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .context("new service did not reach expected readiness within 60 seconds")?;
    Ok(())
}

fn record_result(paths: &Paths, job: &Job) -> Result<()> {
    journal::write_json(&paths.job_dir(job)?.join("result.json"), job)?;
    if job.phase != Phase::Succeeded {
        return Ok(());
    }
    // Keep the latest successful rollback snapshot; failed/recovery directories are never pruned.
    for entry in std::fs::read_dir(&paths.root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || entry.file_name() == std::ffi::OsStr::new(&job.id)
            || uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err()
        {
            continue;
        }
        if journal::read_json::<Job>(&entry.path().join("result.json"))?
            .is_some_and(|j| j.phase == Phase::Succeeded)
        {
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    for entry in std::fs::read_dir(paths.job_dir(job)?)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_file()
            && (name.starts_with("candidate-") || name.starts_with("download-"))
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&p).unwrap();
        p
    }
    fn archive(path: &Path, names: &[(&str, tar::EntryType)]) {
        let gzip = flate2::write::GzEncoder::new(
            std::fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(gzip);
        for (name, kind) in names {
            let mut header = tar::Header::new_gnu();
            header.set_size(20);
            header.set_mode(0o700);
            header.set_entry_type(*kind);
            header.set_cksum();
            let mut data = [0u8; 20];
            data[..4].copy_from_slice(b"\x7fELF");
            data[4] = 2;
            data[5] = 1;
            data[18] = 62;
            tar.append_data(&mut header, name, &data[..]).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }
    #[test]
    fn extractor_accepts_binary_but_rejects_links_duplicates_and_unexpected_paths() {
        for (names, ok) in [
            (vec![("model-bridge", tar::EntryType::Regular)], true),
            (vec![("model-bridge", tar::EntryType::Symlink)], false),
            (
                vec![("nested/model-bridge", tar::EntryType::Regular)],
                false,
            ),
            (
                vec![
                    ("model-bridge", tar::EntryType::Regular),
                    ("model-bridge", tar::EntryType::Regular),
                ],
                false,
            ),
        ] {
            let dir = directory();
            archive(&dir.join("archive"), &names);
            assert_eq!(
                extract(&dir.join("archive"), &dir.join("binary")).is_ok(),
                ok
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn restoring_database_quarantines_new_wal_and_never_runs_after_commit() {
        let dir = directory();
        let id = uuid::Uuid::new_v4().to_string();
        std::fs::create_dir(dir.join(&id)).unwrap();
        let paths = Paths {
            root: dir.clone(),
            install: super::super::Installation {
                binary: dir.join("binary"),
                config: dir.join("config"),
                working_directory: dir.clone(),
            },
            database: dir.join("database"),
            admin_url: "".into(),
        };
        std::fs::write(dir.join(&id).join("database.backup"), b"old").unwrap();
        std::fs::write(&paths.database, b"new").unwrap();
        std::fs::write(dir.join("database-wal"), b"newwal").unwrap();
        let mut job = Job {
            id,
            version: "1.0.1".into(),
            old_version: "1.0.0".into(),
            old_sha256: "".into(),
            phase: Phase::RollingBack,
            backup_complete: true,
            database_sha256: None,
            committed: false,
            rollback_committed: false,
            error: None,
        };
        restore_database(&paths, &job).unwrap();
        assert_eq!(std::fs::read(&paths.database).unwrap(), b"old");
        assert!(!dir.join("database-wal").exists());
        job.committed = true;
        std::fs::write(&paths.database, b"new business").unwrap();
        assert!(restore_database(&paths, &job).is_err());
        assert_eq!(std::fs::read(&paths.database).unwrap(), b"new business");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod transaction_tests;
