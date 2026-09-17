pub mod backup;
pub mod journal;
pub mod lifecycle;
pub mod release;
pub mod worker;

use crate::config::{AppConfig, Cli};
use anyhow::{ensure, Context, Result};
use journal::{Job, Phase};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
pub struct Installation {
    pub binary: PathBuf,
    pub config: PathBuf,
    pub working_directory: PathBuf,
}
#[derive(Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub install: Installation,
    pub database: PathBuf,
    pub admin_url: String,
}
impl Paths {
    pub fn job_path(&self) -> PathBuf {
        self.root.join("job.json")
    }
    pub fn job(&self) -> Result<Option<Job>> {
        let job: Option<Job> = journal::read_json(&self.job_path())?;
        // Readers can observe rename before the writer's directory fsync. Persist the observed
        // commit before any caller admits traffic; otherwise a crash could undo its boundary.
        if job
            .as_ref()
            .is_some_and(|j| j.committed || j.rollback_committed)
        {
            std::fs::File::open(self.job_path())?.sync_all()?;
            journal::sync_dir(&self.root)?;
        }
        Ok(job)
    }
    pub fn save(&self, job: &Job) -> Result<()> {
        journal::write_json(&self.job_path(), job)
    }
    pub fn job_dir(&self, job: &Job) -> Result<PathBuf> {
        uuid::Uuid::parse_str(&job.id).context("invalid update job ID")?;
        Ok(self.root.join(&job.id))
    }
    pub fn lock(&self) -> Result<std::fs::File> {
        journal::private_dir(&self.root)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("worker.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file).context("update worker is busy")?;
        Ok(file)
    }
}
fn root() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("home directory unavailable")?
        .join(".local/share/model-bridge/update"))
}

pub fn check_install() -> Result<()> {
    if let Some(job) = journal::read_json::<Job>(&root()?.join("job.json"))? {
        ensure!(job.terminal(), "resolve pending update before reinstalling");
    }
    Ok(())
}
pub fn register(cli: &Cli) -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "installation registration requires Linux"
    );
    let root = root()?;
    let installation = Installation {
        binary: std::env::current_exe()?.canonicalize()?,
        config: cli.config.canonicalize()?,
        working_directory: std::env::current_dir()?.canonicalize()?,
    };
    ensure!(
        installation.working_directory.join("update") == root,
        "registration must run in the installed data directory"
    );
    if let Some(job) = journal::read_json::<Job>(&root.join("job.json"))? {
        ensure!(job.terminal(), "resolve pending update before reinstalling");
    }
    journal::write_json(&root.join("install.json"), &installation)?;
    // Manual installation establishes a new baseline; completed jobs must not pin startup to an older version.
    if let Some(job) = journal::read_json::<Job>(&root.join("job.json"))? {
        uuid::Uuid::parse_str(&job.id)?;
        journal::write_json(&root.join(&job.id).join("result.json"), &job)?;
        std::fs::remove_file(root.join("job.json"))?;
        journal::sync_dir(&root)?;
    }
    Ok(())
}
pub fn installed_paths(cli: &Cli, config: &AppConfig) -> Result<Paths> {
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "仅支持 Linux x86_64"
    );
    ensure!(
        matches!(
            config.admin.host.as_str(),
            "127.0.0.1" | "localhost" | "::1"
        ),
        "管理服务必须绑定 loopback"
    );
    let root = root()?;
    let install: Installation = journal::read_json(&root.join("install.json"))?
        .context("请先运行新版 scripts/install-user.sh 安装更新服务")?;
    ensure!(
        cli.config.canonicalize()? == install.config
            && std::env::current_dir()?.canonicalize()? == install.working_directory,
        "配置或工作目录与安装记录不一致"
    );
    ensure!(
        install.binary.is_absolute() && install.working_directory.join("update") == root,
        "invalid installation paths"
    );
    let options: sqlx::sqlite::SqliteConnectOptions = config.database.path.parse()?;
    let database = options.get_filename();
    ensure!(
        database != Path::new(":memory:"),
        "updates require an on-disk database"
    );
    let database = if database.is_absolute() {
        database.to_path_buf()
    } else {
        install.working_directory.join(database)
    };
    let host = if config.admin.host == "::1" {
        "[::1]"
    } else {
        config.admin.host.as_str()
    };
    Ok(Paths {
        root,
        install,
        database,
        admin_url: format!("http://{host}:{}", config.admin.port),
    })
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Check {
    pub checked_at: Option<i64>,
    pub candidate: Option<release::ReleaseInfo>,
    pub error: Option<String>,
}
pub struct Manager {
    pub paths: Option<Paths>,
    pub unsupported_reason: Option<String>,
    pub active: AtomicBool,
    pub ready: AtomicBool,
    pub checking: AtomicBool,
    pub check: Mutex<Check>,
    pub startup_job: Option<Job>,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            paths: None,
            unsupported_reason: Some("此实例未通过用户级 systemd 安装".into()),
            active: AtomicBool::new(true),
            ready: AtomicBool::new(false),
            checking: AtomicBool::new(false),
            check: Mutex::new(Check::default()),
            startup_job: None,
        }
    }
}
impl Manager {
    pub fn bootstrap(cli: &Cli, config: &AppConfig) -> Result<Arc<Self>> {
        if std::env::var("MODEL_BRIDGE_MANAGED").as_deref() != Ok("1") {
            if std::env::current_dir()?.join("update") == root()? {
                let job = journal::read_json::<Job>(&root()?.join("job.json"))?;
                ensure!(
                    job.is_none_or(|j| j.terminal()),
                    "pending update requires the managed service; recover it first"
                );
            }
            return Ok(Arc::new(Self::default()));
        }
        let paths = match installed_paths(cli, config) {
            Ok(p) => p,
            Err(e) => {
                let pending = journal::read_json::<Job>(&root()?.join("job.json"))?;
                ensure!(
                    pending.is_none_or(|j| j.terminal()),
                    "pending update and invalid installation: {e}"
                );
                return Ok(Arc::new(Self {
                    unsupported_reason: Some(e.to_string()),
                    ..Self::default()
                }));
            }
        };
        ensure!(
            std::env::current_exe()?.canonicalize()? == paths.install.binary,
            "running binary does not match installation"
        );
        let job = paths.job()?;
        let active = match &job {
            None => true,
            Some(j) if j.permits_traffic(env!("CARGO_PKG_VERSION")) => true,
            Some(j)
                if (j.phase == Phase::Validating && j.version == env!("CARGO_PKG_VERSION"))
                    || (j.phase == Phase::RollbackValidating
                        && j.old_version == env!("CARGO_PKG_VERSION")) =>
            {
                false
            }
            Some(_) => anyhow::bail!(
                "unfinished update: run systemctl --user start model-bridge-update to recover"
            ),
        };
        let mut check: Check =
            journal::read_json(&paths.root.join("check.json"))?.unwrap_or_default();
        if check.candidate.as_ref().is_some_and(|r| {
            semver::Version::parse(&r.version).ok()
                <= semver::Version::parse(env!("CARGO_PKG_VERSION")).ok()
        }) {
            check.candidate = None;
        }
        Ok(Arc::new(Self {
            paths: Some(paths),
            unsupported_reason: None,
            active: AtomicBool::new(active),
            ready: AtomicBool::new(false),
            checking: AtomicBool::new(false),
            check: Mutex::new(check),
            startup_job: job,
        }))
    }
    pub fn validating(&self) -> bool {
        !self.active.load(Ordering::Acquire)
    }
    pub async fn wait_active(&self) {
        while !self.active.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    pub fn watch_commit(self: &Arc<Self>, stop: tokio_util::sync::CancellationToken) {
        let this = self.clone();
        tokio::spawn(async move {
            while this.validating() {
                tokio::select! {_=stop.cancelled()=>return, _=tokio::time::sleep(Duration::from_millis(200))=>{}}
                if let Some(paths) = &this.paths {
                    if let Ok(Some(job)) = paths.job() {
                        if this
                            .startup_job
                            .as_ref()
                            .is_some_and(|old| old.id == job.id)
                            && job.permits_traffic(env!("CARGO_PKG_VERSION"))
                        {
                            this.active.store(true, Ordering::Release);
                        }
                    }
                }
            }
        });
    }
    pub async fn start_check(self: &Arc<Self>) -> Result<()> {
        let mut check = self.check.lock().await;
        let now = chrono::Utc::now().timestamp();
        ensure!(!self.checking.load(Ordering::Acquire), "正在检查版本");
        ensure!(
            check.checked_at.is_none_or(|t| now - t >= 60),
            "请等待 60 秒后再次检查"
        );
        check.checked_at = Some(now);
        self.checking.store(true, Ordering::Release);
        let this = self.clone();
        tokio::spawn(async move {
            let result = async {
                release::check_release(&release::client()?, env!("CARGO_PKG_VERSION")).await
            }
            .await;
            let mut check = this.check.lock().await;
            match result {
                Ok(candidate) => {
                    check.candidate = candidate;
                    check.error = None;
                }
                Err(e) => {
                    check.candidate = None;
                    check.error = Some(e.to_string());
                }
            }
            if let Some(paths) = &this.paths {
                if let Err(e) = journal::write_json(&paths.root.join("check.json"), &*check) {
                    check.error = Some(e.to_string());
                }
            }
            this.checking.store(false, Ordering::Release);
        });
        Ok(())
    }
    pub async fn schedule(self: &Arc<Self>, stop: tokio_util::sync::CancellationToken) {
        let jitter = (uuid::Uuid::new_v4().as_u128() % 60) as u64;
        let this = self.clone();
        tokio::spawn(async move {
            tokio::select! {_=stop.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs(60+jitter))=>{}}
            loop {
                this.wait_active().await;
                let _ = this.start_check().await;
                tokio::select! {_=stop.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs(86400))=>{}}
            }
        });
    }
    pub async fn apply(&self, version: &str) -> Result<String> {
        let paths = self.paths.as_ref().context("此安装方式不支持自动更新")?;
        if let Some(job) = paths.job()? {
            if !job.terminal() {
                ensure!(job.version == version, "另一版本正在更新");
                return Ok(job.id);
            }
        }
        let _lock = paths.lock()?;
        if let Some(job) = paths.job()? {
            ensure!(job.terminal(), "更新任务已存在");
        }
        let candidate = self
            .check
            .lock()
            .await
            .candidate
            .clone()
            .context("请先成功检查新版本")?;
        ensure!(candidate.version == version, "目标版本与已检查版本不一致");
        ensure!(
            semver::Version::parse(version)? > semver::Version::parse(env!("CARGO_PKG_VERSION"))?,
            "不允许降级"
        );
        let pid = worker::systemctl(&[
            "show",
            "model-bridge.service",
            "--property=MainPID",
            "--value",
        ])
        .await?;
        ensure!(
            pid.trim() == std::process::id().to_string(),
            "当前进程不是安装的 systemd 主服务"
        );
        worker::systemctl(&[
            "show",
            "model-bridge-update.service",
            "--property=LoadState",
            "--value",
        ])
        .await
        .and_then(|s| {
            ensure!(s.trim() == "loaded", "更新服务未安装");
            Ok(())
        })?;
        backup::atomic_copy(&paths.install.binary, &paths.root.join("worker"), true)?;
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            version: version.into(),
            old_version: env!("CARGO_PKG_VERSION").into(),
            old_sha256: backup::digest(&paths.install.binary)?,
            phase: Phase::Downloading,
            backup_complete: false,
            database_sha256: None,
            committed: false,
            rollback_committed: false,
            error: None,
        };
        journal::private_dir(&paths.job_dir(&job)?)?;
        journal::write_json(&paths.job_dir(&job)?.join("release.json"), &candidate)?;
        paths.save(&job)?;
        // Release lock before dispatch: worker takes the same lock in its own systemd cgroup.
        drop(_lock);
        if let Err(e) =
            worker::systemctl(&["start", "--no-block", "model-bridge-update.service"]).await
        {
            let mut failed = job.clone();
            failed.phase = Phase::Failed;
            failed.error = Some(e.to_string());
            paths.save(&failed)?;
            return Err(e);
        }
        Ok(job.id)
    }
    pub fn record_drained(&self) -> Result<()> {
        if let Some(paths) = &self.paths {
            if let Some(job) = paths.job()? {
                if job.phase == Phase::Stopping {
                    journal::write_json(&paths.job_dir(&job)?.join("drained.json"), &job.id)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_lock_serializes_installation_across_file_descriptors() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let paths = Paths {
            root: dir.clone(),
            install: Installation {
                binary: dir.join("binary"),
                config: dir.join("config"),
                working_directory: dir.clone(),
            },
            database: dir.join("db"),
            admin_url: String::new(),
        };
        let first = paths.lock().unwrap();
        assert!(paths.lock().is_err());
        drop(first);
        drop(paths.lock().unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn active_job_reuses_id_but_rejects_other_target_without_starting_a_worker() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let paths = Paths {
            root: dir.clone(),
            install: Installation {
                binary: dir.join("binary"),
                config: dir.join("config"),
                working_directory: dir.clone(),
            },
            database: dir.join("db"),
            admin_url: String::new(),
        };
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            version: "1.0.0".into(),
            old_version: env!("CARGO_PKG_VERSION").into(),
            old_sha256: String::new(),
            phase: Phase::Downloading,
            backup_complete: false,
            database_sha256: None,
            committed: false,
            rollback_committed: false,
            error: None,
        };
        paths.save(&job).unwrap();
        let manager = Manager {
            paths: Some(paths),
            ..Manager::default()
        };
        assert_eq!(manager.apply("1.0.0").await.unwrap(), job.id);
        assert!(manager.apply("1.0.1").await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn manual_check_cooldown_does_not_start_network_task() {
        let manager = Arc::new(Manager::default());
        manager.check.lock().await.checked_at = Some(chrono::Utc::now().timestamp());
        assert!(manager.start_check().await.is_err());
        assert!(!manager.checking.load(Ordering::Acquire));
    }
}
