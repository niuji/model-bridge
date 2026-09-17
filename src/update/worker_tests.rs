use super::*;
use sqlx::{sqlite::SqliteConnectOptions, Connection, SqliteConnection};
use std::{cell::RefCell, path::PathBuf};

struct Fixture {
    directory: PathBuf,
    paths: Paths,
    job: Job,
    staged: PathBuf,
}
impl Fixture {
    async fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("mb-transaction-{}", uuid::Uuid::new_v4()));
        let root = directory.join("update");
        journal::private_dir(&root).unwrap();
        let binary = directory.join("model-bridge");
        let staged = directory.join(".staged");
        std::fs::write(&binary, b"old binary").unwrap();
        std::fs::write(&staged, b"new binary").unwrap();
        let paths = Paths {
            root,
            install: super::super::Installation {
                binary,
                config: directory.join("config.toml"),
                working_directory: directory.clone(),
            },
            database: directory.join("database.sqlite"),
            admin_url: "http://127.0.0.1:1".into(),
        };
        let mut db = connect(&paths.database, true).await;
        sqlx::query("CREATE TABLE events(value TEXT NOT NULL)")
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO events VALUES ('original')")
            .execute(&mut db)
            .await
            .unwrap();
        db.close().await.unwrap();
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            version: "1.1.0".into(),
            old_version: "1.0.0".into(),
            old_sha256: backup::digest(&paths.install.binary).unwrap(),
            phase: Phase::Downloading,
            backup_complete: false,
            database_sha256: None,
            committed: false,
            rollback_committed: false,
            error: None,
        };
        journal::private_dir(&paths.job_dir(&job).unwrap()).unwrap();
        paths.save(&job).unwrap();
        Self {
            directory,
            paths,
            job,
            staged,
        }
    }
    fn service(&self) -> FakeService {
        FakeService {
            paths: self.paths.clone(),
            drain: true,
            fail_ready: RefCell::new(None),
        }
    }
    fn binary(&self) -> Vec<u8> {
        std::fs::read(&self.paths.install.binary).unwrap()
    }
    fn saved(&self) -> Job {
        self.paths.job().unwrap().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

async fn connect(path: &Path, create: bool) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(create),
    )
    .await
    .unwrap()
}
async fn values(path: &Path) -> Vec<String> {
    let mut db = connect(path, false).await;
    let values = sqlx::query_scalar("SELECT value FROM events ORDER BY rowid")
        .fetch_all(&mut db)
        .await
        .unwrap();
    db.close().await.unwrap();
    values
}
async fn append(path: &Path, value: &str) {
    let mut db = connect(path, false).await;
    sqlx::query("INSERT INTO events VALUES (?)")
        .bind(value)
        .execute(&mut db)
        .await
        .unwrap();
    db.close().await.unwrap();
}
async fn migration_exists(path: &Path) -> bool {
    let mut db = connect(path, false).await;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name = 'new_schema'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    db.close().await.unwrap();
    count == 1
}

// Only systemd commands and readiness are faked; SQLite snapshots, replacements, journals,
// and recovery all run through the production implementation on real temporary files.
struct FakeService {
    paths: Paths,
    drain: bool,
    fail_ready: RefCell<Option<(Phase, bool)>>,
}
impl Service for FakeService {
    async fn command(&self, args: &[&str]) -> Result<String> {
        let job = self.paths.job()?.unwrap();
        match args[0] {
            "show" => return Ok("0\n".into()),
            "stop" if job.phase == Phase::Stopping && self.drain => {
                journal::write_json(&self.paths.job_dir(&job)?.join("drained.json"), &job.id)?;
            }
            "start" if job.phase == Phase::Validating => {
                let mut db = connect(&self.paths.database, false).await;
                sqlx::query("CREATE TABLE new_schema(value TEXT)")
                    .execute(&mut db)
                    .await?;
                sqlx::query("INSERT INTO events VALUES ('migration')")
                    .execute(&mut db)
                    .await?;
                db.close().await?;
            }
            "stop" | "start" | "reset-failed" => {}
            command => anyhow::bail!("unexpected fake service command {command}"),
        }
        Ok(String::new())
    }
    async fn ready(&self, paths: &Paths, job: &Job, version: &str, active: bool) -> Result<()> {
        let saved = paths.job()?.unwrap();
        assert_eq!(
            saved.phase, job.phase,
            "readiness must follow a durable phase write"
        );
        assert_eq!(saved.committed, job.committed);
        assert_eq!(saved.rollback_committed, job.rollback_committed);
        assert_eq!(
            saved.permits_traffic(version),
            active,
            "readiness must respect commit gates"
        );
        let expected = if std::fs::read(&paths.install.binary)? == b"new binary" {
            &job.version
        } else {
            &job.old_version
        };
        assert_eq!(
            version, expected,
            "readiness must verify the installed version"
        );
        let fail = self
            .fail_ready
            .borrow()
            .as_ref()
            .is_some_and(|(phase, wanted_active)| phase == &job.phase && *wanted_active == active);
        if fail {
            self.fail_ready.borrow_mut().take();
            anyhow::bail!("injected readiness failure");
        }
        Ok(())
    }
}

#[tokio::test]
async fn activation_snapshots_old_state_and_commits_new_binary_and_migrated_database() {
    let mut f = Fixture::new().await;
    let service = f.service();
    activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .unwrap();
    assert_eq!(f.binary(), b"new binary");
    assert_eq!(f.saved().phase, Phase::Succeeded);
    assert!(f.saved().committed);
    let dir = f.paths.job_dir(&f.job).unwrap();
    assert_eq!(
        std::fs::read(dir.join("binary.backup")).unwrap(),
        b"old binary"
    );
    assert_eq!(values(&dir.join("database.backup")).await, ["original"]);
    assert_eq!(values(&f.paths.database).await, ["original", "migration"]);
    assert!(migration_exists(&f.paths.database).await);
    assert!(!f.staged.exists());
}

#[tokio::test]
async fn validation_failure_restores_binary_database_contents_and_schema() {
    let mut f = Fixture::new().await;
    let service = f.service();
    *service.fail_ready.borrow_mut() = Some((Phase::Validating, false));
    assert!(activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .is_err());
    assert!(migration_exists(&f.paths.database).await);
    recover(&f.paths, &mut f.job, &service).await.unwrap();
    assert_eq!(f.binary(), b"old binary");
    assert_eq!(values(&f.paths.database).await, ["original"]);
    assert!(!migration_exists(&f.paths.database).await);
    assert_eq!(f.saved().phase, Phase::RolledBack);
    assert!(f.saved().rollback_committed);
    assert!(!f.saved().committed);
}

#[tokio::test]
async fn failed_drain_preserves_database_and_marks_failed_only_after_old_readiness() {
    let mut f = Fixture::new().await;
    let mut service = f.service();
    service.drain = false;
    assert!(activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .is_err());
    assert!(!f.saved().backup_complete);
    assert_eq!(f.saved().phase, Phase::Stopping);
    *service.fail_ready.borrow_mut() = Some((Phase::RestartingOld, true));
    assert!(recover(&f.paths, &mut f.job, &service).await.is_err());
    assert_eq!(f.saved().phase, Phase::RestartingOld);
    assert!(!f.saved().terminal());
    recover(&f.paths, &mut f.job, &service).await.unwrap();
    assert_eq!(f.saved().phase, Phase::Failed);
    assert_eq!(f.binary(), b"old binary");
    assert_eq!(values(&f.paths.database).await, ["original"]);
}

#[tokio::test]
async fn recovery_after_commit_preserves_new_business_writes() {
    let mut f = Fixture::new().await;
    let service = f.service();
    *service.fail_ready.borrow_mut() = Some((Phase::Committed, true));
    assert!(activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .is_err());
    assert!(f.saved().committed);
    append(&f.paths.database, "new business write").await;
    f.job.phase = Phase::RecoveryRequired;
    f.paths.save(&f.job).unwrap();
    let mut restarted_job = f.saved();
    recover(&f.paths, &mut restarted_job, &service)
        .await
        .unwrap();
    assert_eq!(f.binary(), b"new binary");
    assert_eq!(
        values(&f.paths.database).await,
        ["original", "migration", "new business write"]
    );
    assert_eq!(f.saved().phase, Phase::Succeeded);
}

#[tokio::test]
async fn recovery_required_after_restart_uses_durable_snapshot() {
    let mut f = Fixture::new().await;
    let service = f.service();
    *service.fail_ready.borrow_mut() = Some((Phase::Validating, false));
    assert!(activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .is_err());
    f.job.phase = Phase::RecoveryRequired;
    f.paths.save(&f.job).unwrap();
    let mut restarted_job = f.saved();
    recover(&f.paths, &mut restarted_job, &f.service())
        .await
        .unwrap();
    assert_eq!(f.binary(), b"old binary");
    assert_eq!(values(&f.paths.database).await, ["original"]);
    assert_eq!(f.saved().phase, Phase::RolledBack);
}

#[tokio::test]
async fn rollback_commit_retries_never_restore_snapshot_over_later_writes() {
    let mut f = Fixture::new().await;
    let service = f.service();
    *service.fail_ready.borrow_mut() = Some((Phase::Validating, false));
    assert!(activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .is_err());
    *service.fail_ready.borrow_mut() = Some((Phase::RollbackValidating, true));
    assert!(recover(&f.paths, &mut f.job, &service).await.is_err());
    assert!(f.saved().rollback_committed);
    append(&f.paths.database, "write after rollback commit").await;
    for _ in 0..2 {
        let mut restarted_job = f.saved();
        recover(&f.paths, &mut restarted_job, &service)
            .await
            .unwrap();
        assert_eq!(
            values(&f.paths.database).await,
            ["original", "write after rollback commit"]
        );
    }
    assert_eq!(f.saved().phase, Phase::RolledBack);
}

#[tokio::test]
async fn rollback_quarantines_real_uncheckpointed_wal_before_restoring_snapshot() {
    let mut f = Fixture::new().await;
    let service = f.service();
    *service.fail_ready.borrow_mut() = Some((Phase::Validating, false));
    assert!(activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .is_err());
    let live = f.directory.join("live.sqlite");
    let mut db = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&live)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
    )
    .await
    .unwrap();
    sqlx::query("PRAGMA wal_autocheckpoint=0")
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE events(value TEXT)")
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO events VALUES ('uncheckpointed replacement')")
        .execute(&mut db)
        .await
        .unwrap();
    // Copy a committed WAL database while its connection is open, reproducing files left by a crash.
    for suffix in ["", "-wal", "-shm"] {
        std::fs::copy(
            format!("{}{suffix}", live.display()),
            format!("{}{suffix}", f.paths.database.display()),
        )
        .unwrap();
    }
    let wal = std::fs::read(format!("{}-wal", f.paths.database.display())).unwrap();
    assert!(!wal.is_empty());
    db.close().await.unwrap();
    recover(&f.paths, &mut f.job, &service).await.unwrap();
    assert_eq!(values(&f.paths.database).await, ["original"]);
    let quarantine = std::fs::read_dir(&f.directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("model-bridge-failed-")
        })
        .unwrap();
    assert_eq!(std::fs::read(quarantine.join("database-wal")).unwrap(), wal);
}

#[tokio::test]
async fn successful_result_prunes_only_previous_successes_and_transient_downloads() {
    let mut f = Fixture::new().await;
    let mut previous = f.job.clone();
    previous.id = uuid::Uuid::new_v4().to_string();
    previous.phase = Phase::Succeeded;
    let old_success = f.paths.job_dir(&previous).unwrap();
    journal::write_json(&old_success.join("result.json"), &previous).unwrap();
    let mut failed = previous.clone();
    failed.id = uuid::Uuid::new_v4().to_string();
    failed.phase = Phase::Failed;
    let old_failure = f.paths.job_dir(&failed).unwrap();
    journal::write_json(&old_failure.join("result.json"), &failed).unwrap();
    let unresolved = f.paths.root.join(uuid::Uuid::new_v4().to_string());
    journal::private_dir(&unresolved).unwrap();
    let service = f.service();
    activate(&f.paths, &mut f.job, &f.staged, &service)
        .await
        .unwrap();
    let current = f.paths.job_dir(&f.job).unwrap();
    std::fs::write(current.join("candidate-unused"), b"candidate").unwrap();
    std::fs::write(current.join("download-unused.tar.gz"), b"archive").unwrap();
    record_result(&f.paths, &f.job).unwrap();
    assert!(!old_success.exists());
    assert!(old_failure.join("result.json").exists());
    assert!(unresolved.exists());
    assert!(current.join("binary.backup").exists());
    assert!(current.join("database.backup").exists());
    assert!(!current.join("candidate-unused").exists());
    assert!(!current.join("download-unused.tar.gz").exists());
}
