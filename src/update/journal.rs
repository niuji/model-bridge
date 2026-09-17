use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Downloading,
    Stopping,
    BackedUp,
    Validating,
    Committed,
    Succeeded,
    RollingBack,
    RollbackValidating,
    RolledBack,
    Failed,
    RecoveryRequired,
    RestartingOld,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub version: String,
    pub old_version: String,
    pub old_sha256: String,
    pub phase: Phase,
    pub backup_complete: bool,
    pub database_sha256: Option<String>,
    pub committed: bool,
    pub rollback_committed: bool,
    pub error: Option<String>,
}
impl Job {
    pub fn terminal(&self) -> bool {
        matches!(
            self.phase,
            Phase::Succeeded | Phase::RolledBack | Phase::Failed
        )
    }
    pub fn permits_traffic(&self, version: &str) -> bool {
        if self.committed {
            return version == self.version;
        }
        if self.rollback_committed {
            return version == self.old_version;
        }
        matches!(
            self.phase,
            Phase::Downloading | Phase::Failed | Phase::RestartingOld
        ) && version == self.old_version
    }
}
pub fn sync_dir(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
}
pub fn private_dir(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing parent"))?;
    private_dir(parent)?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        sync_dir(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    pub fn job() -> Job {
        Job {
            id: "test-job".into(),
            version: "0.6.0".into(),
            old_version: "0.5.21".into(),
            old_sha256: "a".repeat(64),
            phase: Phase::Validating,
            backup_complete: true,
            database_sha256: None,
            committed: false,
            rollback_committed: false,
            error: None,
        }
    }
    #[test]
    fn traffic_requires_durable_commit_and_matching_version() {
        let mut j = job();
        assert!(!j.permits_traffic("0.6.0"));
        j.committed = true;
        j.phase = Phase::Committed;
        assert!(j.permits_traffic("0.6.0"));
        assert!(!j.permits_traffic("0.5.21"));
        j.phase = Phase::RecoveryRequired;
        assert!(j.permits_traffic("0.6.0"));
    }
    #[test]
    fn rollback_only_opens_old_version_after_commit() {
        let mut j = job();
        j.phase = Phase::RollbackValidating;
        assert!(!j.permits_traffic("0.5.21"));
        j.rollback_committed = true;
        assert!(j.permits_traffic("0.5.21"));
        assert!(!j.permits_traffic("0.6.0"));
    }
    #[test]
    fn journal_survives_replace_and_corruption_is_not_absence() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("job.json");
        let mut j = job();
        write_json(&path, &j).unwrap();
        j.committed = true;
        write_json(&path, &j).unwrap();
        assert!(read_json::<Job>(&path).unwrap().unwrap().committed);
        std::fs::write(&path, b"{").unwrap();
        assert!(read_json::<Job>(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn restarting_old_remains_resumable_until_readiness() {
        let mut job = super::tests::job();
        job.backup_complete = false;
        job.phase = Phase::RestartingOld;
        assert!(!job.terminal());
        assert!(job.permits_traffic("0.5.21"));
        assert!(!job.permits_traffic("0.6.0"));
        job.phase = Phase::Failed;
        assert!(job.terminal());
    }
}
