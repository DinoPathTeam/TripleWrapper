//! Local integrity database.
//!
//! Append-only JSON-lines log in `$XDG_DATA_HOME/triplewrapper/integrity.log`.
//! Records the BLAKE3 of every successfully processed archive so later runs
//! can prove "this file is bit-identical to the one we verified before".
//! No network, no daemon: plain local file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::checksum::compute_archive_checksum;
use crate::types::ChecksumAlgorithm;
use crate::{Result, TripleWrapperError};

/// One verified state of an archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityRecord {
    pub archive_path: String,
    pub size_bytes: u64,
    pub mtime_secs: u64,
    pub blake3: String,
    pub operation: String,
    pub recorded_at: u64,
}

/// Local append-only integrity log.
pub struct IntegrityDb {
    path: PathBuf,
}

impl IntegrityDb {
    /// Open (creating parent dirs) the log at the XDG data dir.
    pub fn open() -> Result<Self> {
        let path = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("triplewrapper")
            .join("integrity.log");
        Self::open_at(path)
    }

    /// Open at an explicit path (used by tests).
    pub fn open_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { path })
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Hash `archive` with BLAKE3 and append a record. Returns the record.
    pub async fn record(&self, archive: &Path, operation: &str) -> Result<IntegrityRecord> {
        let meta = std::fs::metadata(archive)?;
        let blake3 = compute_archive_checksum(archive, ChecksumAlgorithm::Blake3).await?;
        let record = IntegrityRecord {
            archive_path: archive.to_string_lossy().into_owned(),
            size_bytes: meta.len(),
            mtime_secs: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            blake3,
            operation: operation.to_string(),
            recorded_at: Self::now_secs(),
        };
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        self.append(&line).await?;
        Ok(record)
    }

    /// Last record for `archive`, if any.
    pub async fn last_for(&self, archive: &Path) -> Result<Option<IntegrityRecord>> {
        let key = archive.to_string_lossy();
        let mut last = None;
        for record in self.read_all().await? {
            if record.archive_path == key {
                last = Some(record);
            }
        }
        Ok(last)
    }

    /// All records, oldest first.
    pub async fn list(&self) -> Result<Vec<IntegrityRecord>> {
        self.read_all().await
    }

    /// Verify current file against its last record.
    /// Returns `(matches, current_record, previous_record)`.
    pub async fn check(
        &self,
        archive: &Path,
    ) -> Result<(bool, IntegrityRecord, Option<IntegrityRecord>)> {
        let previous = self.last_for(archive).await?;
        let meta = std::fs::metadata(archive)?;
        let blake3 = compute_archive_checksum(archive, ChecksumAlgorithm::Blake3).await?;
        let current = IntegrityRecord {
            archive_path: archive.to_string_lossy().into_owned(),
            size_bytes: meta.len(),
            mtime_secs: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            blake3,
            operation: "check".to_string(),
            recorded_at: Self::now_secs(),
        };
        let matches = previous
            .as_ref()
            .is_some_and(|p| p.blake3 == current.blake3 && p.size_bytes == current.size_bytes);
        Ok((matches, current, previous))
    }

    async fn append(&self, line: &[u8]) -> Result<()> {
        use tokio::fs::OpenOptions;
        use tokio::io::AsyncWriteExt;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(line).await?;
        Ok(())
    }

    async fn read_all(&self) -> Result<Vec<IntegrityRecord>> {
        let data = match tokio::fs::read(&self.path).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(TripleWrapperError::Io(e)),
        };
        let mut out = Vec::new();
        for line in data.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            // Skip corrupt lines instead of failing the whole history.
            if let Ok(record) = serde_json::from_slice(line) {
                out.push(record);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    async fn fixture(dir: &std::path::Path) -> PathBuf {
        let file = dir.join("archive.bin");
        let mut f = File::create(&file).await.unwrap();
        f.write_all(b"triplewrapper-integrity-fixture")
            .await
            .unwrap();
        f.flush().await.unwrap();
        file
    }

    #[tokio::test]
    async fn test_record_and_check_match() {
        let dir = tempdir().unwrap();
        let db = IntegrityDb::open_at(dir.path().join("integrity.log")).unwrap();
        let archive = fixture(dir.path()).await;

        let record = db.record(&archive, "test").await.unwrap();
        assert_eq!(record.blake3.len(), 64);

        let (matches, _, previous) = db.check(&archive).await.unwrap();
        assert!(matches);
        assert!(previous.is_some());
    }

    #[tokio::test]
    async fn test_check_mismatch_after_modification() {
        let dir = tempdir().unwrap();
        let db = IntegrityDb::open_at(dir.path().join("integrity.log")).unwrap();
        let archive = fixture(dir.path()).await;

        db.record(&archive, "test").await.unwrap();

        let mut f = File::create(&archive).await.unwrap();
        f.write_all(b"tampered").await.unwrap();
        f.flush().await.unwrap();

        let (matches, _, _) = db.check(&archive).await.unwrap();
        assert!(!matches);
    }

    #[tokio::test]
    async fn test_check_without_history() {
        let dir = tempdir().unwrap();
        let db = IntegrityDb::open_at(dir.path().join("integrity.log")).unwrap();
        let archive = fixture(dir.path()).await;

        let (matches, _, previous) = db.check(&archive).await.unwrap();
        assert!(!matches);
        assert!(previous.is_none());
    }

    #[tokio::test]
    async fn test_corrupt_lines_are_skipped() {
        let dir = tempdir().unwrap();
        let log = dir.path().join("integrity.log");
        std::fs::write(&log, b"not json\n{\"archive_path\":").unwrap();
        let db = IntegrityDb::open_at(log).unwrap();
        assert!(db.list().await.unwrap().is_empty());
    }
}
