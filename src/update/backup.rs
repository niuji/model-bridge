use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqliteConnectOptions, Connection};
use std::path::Path;

pub fn digest(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub async fn snapshot(source: &Path, destination: &Path) -> Result<()> {
    anyhow::ensure!(!destination.exists(), "backup already exists");
    let options = SqliteConnectOptions::new()
        .filename(source)
        .create_if_missing(false);
    let mut connection = sqlx::SqliteConnection::connect_with(&options).await?;
    let result = sqlx::query("VACUUM INTO ?")
        .bind(destination.to_str().context("non UTF-8 database path")?)
        .execute(&mut connection)
        .await;
    connection.close().await?;
    result?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))?;
    }
    let mut copy = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(destination)
            .read_only(true),
    )
    .await?;
    let check: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut copy)
        .await?;
    copy.close().await?;
    anyhow::ensure!(check == "ok", "database integrity check failed: {check}");
    std::fs::File::open(destination)?.sync_all()?;
    super::journal::sync_dir(destination.parent().context("missing backup parent")?)?;
    Ok(())
}
pub fn atomic_copy(source: &Path, destination: &Path, executable: bool) -> Result<()> {
    let parent = destination.parent().context("missing destination parent")?;
    let temp = parent.join(format!(".mb-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o700 } else { 0o600 });
        }
        let mut out = options.open(&temp)?;
        std::io::copy(&mut std::fs::File::open(source)?, &mut out)?;
        out.sync_all()?;
        std::fs::rename(&temp, destination)?;
        super::journal::sync_dir(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn snapshot_includes_uncheckpointed_wal_and_does_not_change_source() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("db");
        let backup = dir.join("backup");
        let options = SqliteConnectOptions::new()
            .filename(&source)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let mut connection = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE sample(value TEXT)")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sample VALUES ('secret')")
            .execute(&mut connection)
            .await
            .unwrap();
        snapshot(&source, &backup).await.unwrap();
        let mut copy =
            sqlx::SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&backup))
                .await
                .unwrap();
        let value: String = sqlx::query_scalar("SELECT value FROM sample")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(value, "secret");
        assert!(snapshot(&source, &backup).await.is_err());
        copy.close().await.unwrap();
        connection.close().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn atomic_copy_replaces_without_modifying_open_old_inode() {
        use std::io::Read;
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&dir).unwrap();
        let old = dir.join("app");
        let new = dir.join("new");
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        let mut running = std::fs::File::open(&old).unwrap();
        atomic_copy(&new, &old, true).unwrap();
        let mut bytes = Vec::new();
        running.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"old");
        assert_eq!(std::fs::read(&old).unwrap(), b"new");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
