use super::journal;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

pub fn read(directory: &Path) -> Option<DownloadProgress> {
    // Display data must never prevent reading the durable update transaction.
    let progress: DownloadProgress =
        journal::read_json(&directory.join("progress.json")).ok()??;
    (progress.total_bytes > 0 && progress.downloaded_bytes <= progress.total_bytes)
        .then_some(progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_scoped_to_job_and_missing_or_invalid_data_is_optional() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let old_job = root.join("old");
        let current_job = root.join("current");
        journal::private_dir(&current_job).unwrap();
        journal::write_json(
            &old_job.join("progress.json"),
            &DownloadProgress {
                downloaded_bytes: 7,
                total_bytes: 7,
            },
        )
        .unwrap();
        assert!(read(&current_job).is_none());
        let path = current_job.join("progress.json");
        std::fs::write(&path, b"{").unwrap();
        assert!(read(&current_job).is_none());
        for (downloaded_bytes, total_bytes, valid) in [(0, 0, false), (8, 7, false), (3, 7, true)] {
            journal::write_json(
                &path,
                &DownloadProgress {
                    downloaded_bytes,
                    total_bytes,
                },
            )
            .unwrap();
            let progress = read(&current_job);
            assert_eq!(progress.is_some(), valid);
            if let Some(progress) = progress {
                assert_eq!(progress.downloaded_bytes, 3);
                assert_eq!(progress.total_bytes, 7);
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
