//! Checksum verification for archive integrity

use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt};
use blake3;
use sha2::{Digest, Sha256};
use xxhash_rust::xxh3::Xxh3;
use tracing::debug;

use crate::types::ChecksumAlgorithm;
use crate::{Result, TripleWrapperError};

/// Streaming hasher for large files
pub struct StreamingHasher {
    algorithm: ChecksumAlgorithm,
    hasher: Box<dyn HasherImpl>,
    bytes_processed: u64,
}

trait HasherImpl: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize(&mut self) -> Vec<u8>;
    fn algorithm_name(&self) -> &'static str;
}

struct Blake3Hasher {
    hasher: blake3::Hasher,
}

impl HasherImpl for Blake3Hasher {
    fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    fn finalize(&mut self) -> Vec<u8> {
        self.hasher.finalize().as_bytes().to_vec()
    }

    fn algorithm_name(&self) -> &'static str {
        "BLAKE3"
    }
}

struct Sha256Hasher {
    hasher: Sha256,
}

impl HasherImpl for Sha256Hasher {
    fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    fn finalize(&mut self) -> Vec<u8> {
        let hasher = std::mem::replace(&mut self.hasher, Sha256::new());
        hasher.finalize().to_vec()
    }

    fn algorithm_name(&self) -> &'static str {
        "SHA-256"
    }
}

struct Xxh3Hasher {
    hasher: Xxh3,
}

impl HasherImpl for Xxh3Hasher {
    fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    fn finalize(&mut self) -> Vec<u8> {
        self.hasher.digest().to_le_bytes().to_vec()
    }

    fn algorithm_name(&self) -> &'static str {
        "XXH3"
    }
}

impl StreamingHasher {
    pub fn new(algorithm: ChecksumAlgorithm) -> Self {
        let hasher: Box<dyn HasherImpl> = match algorithm {
            ChecksumAlgorithm::Blake3 => Box::new(Blake3Hasher { hasher: blake3::Hasher::new() }),
            ChecksumAlgorithm::Sha256 => Box::new(Sha256Hasher { hasher: Sha256::new() }),
            ChecksumAlgorithm::Xxh3 => Box::new(Xxh3Hasher { hasher: Xxh3::new() }),
        };

        Self {
            algorithm,
            hasher,
            bytes_processed: 0,
        }
    }

    pub fn algorithm(&self) -> ChecksumAlgorithm {
        self.algorithm
    }

    pub fn bytes_processed(&self) -> u64 {
        self.bytes_processed
    }

    /// Update with data chunk
    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
        self.bytes_processed += data.len() as u64;
    }

    /// Finalize and return hex string
    pub fn finalize_hex(&mut self) -> String {
        let hash = self.hasher.finalize();
        hex::encode(hash)
    }

    /// Finalize and return raw bytes
    pub fn finalize(&mut self) -> Vec<u8> {
        self.hasher.finalize()
    }
}

/// Async file hasher with progress reporting
pub struct AsyncFileHasher {
    file: File,
    hasher: StreamingHasher,
    buffer: Vec<u8>,
    buffer_size: usize,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<u64>>,
}

impl AsyncFileHasher {
    pub async fn new(
        path: &Path,
        algorithm: ChecksumAlgorithm,
        buffer_size: Option<usize>,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<u64>>,
    ) -> Result<Self> {
        let file = File::open(path).await?;
        let hasher = StreamingHasher::new(algorithm);
        let buffer_size = buffer_size.unwrap_or(1024 * 1024); // 1 MB default

        Ok(Self {
            file,
            hasher,
            buffer: vec![0u8; buffer_size],
            buffer_size,
            progress_tx,
        })
    }

    /// Compute hash with optional progress reporting
    pub async fn hash(mut self) -> Result<String> {
        loop {
            let n = self.file.read(&mut self.buffer).await?;
            if n == 0 {
                break;
            }
            self.hasher.update(&self.buffer[..n]);

            if let Some(ref tx) = self.progress_tx {
                let _ = tx.send(self.hasher.bytes_processed());
            }
        }

        Ok(self.hasher.finalize_hex())
    }
}

/// Verify file checksum
pub async fn verify_checksum(
    path: &Path,
    expected: &str,
    algorithm: ChecksumAlgorithm,
) -> Result<bool> {
    let hasher = AsyncFileHasher::new(path, algorithm, None, None).await?;
    let actual = hasher.hash().await?;
    Ok(actual.eq_ignore_ascii_case(expected))
}

/// Compute checksum of multiple files (for archive verification)
pub async fn compute_archive_checksum(
    archive_path: &Path,
    algorithm: ChecksumAlgorithm,
) -> Result<String> {
    let hasher = AsyncFileHasher::new(archive_path, algorithm, None, None).await?;
    hasher.hash().await
}

/// Verify archive integrity by computing checksum and comparing
pub async fn verify_archive_integrity(
    archive_path: &Path,
    expected_checksum: Option<&str>,
    algorithm: ChecksumAlgorithm,
) -> Result<bool> {
    let actual = compute_archive_checksum(archive_path, algorithm).await?;
    
    if let Some(expected) = expected_checksum {
        let ok = actual.eq_ignore_ascii_case(expected);
        if !ok {
            tracing::warn!(
                "Checksum mismatch for {}: expected {}, got {}",
                archive_path.display(),
                expected,
                actual
            );
        }
        Ok(ok)
    } else {
        // No expected checksum, just return true after computing
        tracing::debug!("Computed {} for {}: {}", algorithm.name(), archive_path.display(), actual);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_blake3_hash() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        
        let mut f = File::create(&file).await.unwrap();
        f.write_all(b"Hello, TripleWrapper!").await.unwrap();
        f.flush().await.unwrap();

        let hash = compute_archive_checksum(&file, ChecksumAlgorithm::Blake3).await.unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // BLAKE3 = 32 bytes = 64 hex chars
    }

    #[tokio::test]
    async fn test_sha256_hash() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        
        let mut f = File::create(&file).await.unwrap();
        f.write_all(b"Test data").await.unwrap();
        f.flush().await.unwrap();

        let hash = compute_archive_checksum(&file, ChecksumAlgorithm::Sha256).await.unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA256 = 32 bytes = 64 hex chars
    }

    #[tokio::test]
    async fn test_verify_match() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        
        let mut f = File::create(&file).await.unwrap();
        f.write_all(b"Verify me").await.unwrap();
        f.flush().await.unwrap();

        let hash = compute_archive_checksum(&file, ChecksumAlgorithm::Blake3).await.unwrap();
        let ok = verify_checksum(&file, &hash, ChecksumAlgorithm::Blake3).await.unwrap();
        assert!(ok);
    }

    #[tokio::test]
    async fn test_verify_mismatch() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        
        let mut f = File::create(&file).await.unwrap();
        f.write_all(b"Original").await.unwrap();
        f.flush().await.unwrap();

        let ok = verify_checksum(&file, "wrongchecksum", ChecksumAlgorithm::Blake3).await.unwrap();
        assert!(!ok);
    }
}