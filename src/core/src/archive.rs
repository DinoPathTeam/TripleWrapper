//! Archive operations wrapper (7z, tar, pixz)

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info, warn, error};

use crate::types::*;
use crate::{Result, TripleWrapperError};

/// Archive operator for executing 7z/tar/pixz commands
pub struct ArchiveOperator {
    /// Path to 7z binary
    sevenz_path: PathBuf,
    /// Path to tar binary
    tar_path: PathBuf,
    /// Path to pixz binary
    pixz_path: PathBuf,
}

impl ArchiveOperator {
    pub fn new() -> Result<Self> {
        let sevenz_path = Self::find_tool("7z")?;
        let tar_path = Self::find_tool("tar")?;
        let pixz_path = Self::find_tool("pixz").unwrap_or_else(|_| PathBuf::from("pixz"));

        Ok(Self {
            sevenz_path,
            tar_path,
            pixz_path,
        })
    }

    fn find_tool(name: &str) -> Result<PathBuf> {
        let output = std::process::Command::new("which")
            .arg(name)
            .output()
            .map_err(|_| TripleWrapperError::ToolNotFound(name.to_string()))?;

        if !output.status.success() {
            return Err(TripleWrapperError::ToolNotFound(name.to_string()));
        }

        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(PathBuf::from(path))
    }

    /// List archive contents
    pub async fn list(&self, archive: &Path) -> Result<ArchiveMetadata> {
        let format = ArchiveFormat::from_extension(&archive.to_path_buf())
            .ok_or_else(|| TripleWrapperError::InvalidFormat("Unknown archive format".into()))?;

        let entries = match format {
            ArchiveFormat::SevenZ | ArchiveFormat::Zip => self.list_7z(archive).await?,
            ArchiveFormat::Tar | ArchiveFormat::TarGz | ArchiveFormat::TarXz 
                | ArchiveFormat::TarZst | ArchiveFormat::TarBz2 => self.list_tar(archive).await?,
            ArchiveFormat::Pixz => self.list_pixz(archive).await?,
        };

        let size = std::fs::metadata(archive)?.len();

        Ok(ArchiveMetadata {
            path: archive.to_path_buf(),
            format,
            size,
            entries,
            solid: false, // TODO: detect solid archives
            encrypted: false, // TODO: detect encryption
            comment: None,
        })
    }

    async fn list_7z(&self, archive: &Path) -> Result<Vec<ArchiveEntry>> {
        let output = Command::new(&self.sevenz_path)
            .args(["l", "-slt", "-ba", archive.to_str().unwrap()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = output.stdout.ok_or_else(|| TripleWrapperError::CompressionFailed("No stdout".into()))?;
        let mut reader = BufReader::new(stdout).lines();
        
        let mut entries = Vec::new();
        let mut current_entry: Option<ArchiveEntry> = None;

        while let Some(line) = reader.next_line().await? {
            if line.starts_with("----------") {
                if let Some(e) = current_entry.take() {
                    entries.push(e);
                }
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "Path" => {
                        if let Some(e) = current_entry.take() {
                            entries.push(e);
                        }
                        current_entry = Some(ArchiveEntry {
                            path: value.trim().to_string(),
                            size: 0,
                            compressed_size: 0,
                            modified: None,
                            is_directory: value.trim().ends_with('/'),
                            crc32: None,
                            sha256: None,
                        });
                    }
                    "Size" => {
                        if let Some(ref mut e) = current_entry {
                            e.size = value.trim().parse().unwrap_or(0);
                        }
                    }
                    "Packed Size" => {
                        if let Some(ref mut e) = current_entry {
                            e.compressed_size = value.trim().parse().unwrap_or(0);
                        }
                    }
                    "Modified" => {
                        if let Some(ref mut e) = current_entry {
                            // Parse 7z date format
                            e.modified = chrono::DateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S")
                                .ok()
                                .map(|dt| dt.with_timezone(&chrono::Utc));
                        }
                    }
                    "CRC" => {
                        if let Some(ref mut e) = current_entry {
                            e.crc32 = u32::from_str_radix(value.trim(), 16).ok();
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(e) = current_entry {
            entries.push(e);
        }

        Ok(entries)
    }

    async fn list_tar(&self, archive: &Path) -> Result<Vec<ArchiveEntry>> {
        let output = Command::new(&self.tar_path)
            .args(["-tvf", archive.to_str().unwrap()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = output.stdout.ok_or_else(|| TripleWrapperError::CompressionFailed("No stdout".into()))?;
        let mut reader = BufReader::new(stdout).lines();
        let mut entries = Vec::new();

        while let Some(line) = reader.next_line().await? {
            // tar -tv output: -rw-r--r-- user/group size date time path
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let size = parts[3].parse().unwrap_or(0);
                let path = parts[5..].join(" ");
                let is_dir = parts[0].starts_with('d') || path.ends_with('/');
                
                entries.push(ArchiveEntry {
                    path,
                    size,
                    compressed_size: 0, // Unknown for tar
                    modified: None,
                    is_directory: is_dir,
                    crc32: None,
                    sha256: None,
                });
            }
        }

        Ok(entries)
    }

    async fn list_pixz(&self, archive: &Path) -> Result<Vec<ArchiveEntry>> {
        // pixz uses same format as tar for listing
        self.list_tar(archive).await
    }

    /// Extract archive
    pub async fn extract(
        &self,
        archive: &Path,
        output_dir: &Path,
        files: Option<&[String]>,
        workdir: Option<&Path>,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<ProgressTelemetry>>,
    ) -> Result<OperationStats> {
        let format = ArchiveFormat::from_extension(&archive.to_path_buf())
            .ok_or_else(|| TripleWrapperError::InvalidFormat("Unknown archive format".into()))?;

        let start = Instant::now();
        let mut stats = OperationStats {
            duration: std::time::Duration::ZERO,
            bytes_read: 0,
            bytes_written: 0,
            bytes_compressed: 0,
            avg_read_mbps: 0.0,
            avg_write_mbps: 0.0,
            avg_compress_mbps: 0.0,
            peak_memory_mb: 0,
            checksum: None,
        };

        let mut cmd = match format {
            ArchiveFormat::SevenZ | ArchiveFormat::Zip => {
                let mut cmd = Command::new(&self.sevenz_path);
                cmd.args(["x", "-y"]);
                if let Some(wd) = workdir {
                    cmd.arg(format!("-w{}", wd.display()));
                }
                cmd.arg(archive.to_str().unwrap());
                cmd.arg(format!("-o{}", output_dir.display()));
                if let Some(f) = files {
                    for file in f {
                        cmd.arg(file);
                    }
                }
                cmd
            }
            ArchiveFormat::Tar | ArchiveFormat::TarGz | ArchiveFormat::TarXz 
                | ArchiveFormat::TarZst | ArchiveFormat::TarBz2 => {
                let mut cmd = Command::new(&self.tar_path);
                cmd.args(["-xf", archive.to_str().unwrap()]);
                cmd.arg(format!("-C{}", output_dir.display()));
                if let Some(f) = files {
                    for file in f {
                        cmd.arg(file);
                    }
                }
                cmd
            }
            ArchiveFormat::Pixz => {
                let mut cmd = Command::new(&self.pixz_path);
                cmd.args(["-d", "-k"]); // decompress, keep original
                if let Some(wd) = workdir {
                    cmd.arg(format!("-t{}", wd.display()));
                }
                cmd.arg(archive.to_str().unwrap());
                cmd
            }
        };

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("Running: {:?}", cmd);

        let mut child = cmd.spawn()?;

        // Monitor progress from stderr
        let mut bytes_written = 0u64;
        if let Some(stderr) = child.stderr.take() {
            let mut reader = BufReader::new(stderr).lines();
            let mut last_update = Instant::now();

            while let Some(line) = reader.next_line().await? {
                // Parse 7z progress output
                if let Some(progress) = Self::parse_7z_progress(&line) {
                    bytes_written = progress;
                    
                    if last_update.elapsed().as_millis() >= 100 {
                        if let Some(ref tx) = progress_tx {
                            let _ = tx.send(ProgressTelemetry {
                                operation_id: OperationId::new(),
                                status: OperationStatus::Running,
                                current_file: line.clone(),
                                files_total: 0,
                                files_processed: 0,
                                bytes_total: 0,
                                bytes_processed: bytes_written,
                                bytes_per_second_read: 0.0,
                                bytes_per_second_write: 0.0,
                                bytes_per_second_compress: 0.0,
                                eta_seconds: None,
                                cpu_percent: 0.0,
                                memory_bytes: 0,
                            });
                        }
                        last_update = Instant::now();
                    }
                }
            }
        }

        let status = child.wait().await?;
        stats.duration = start.elapsed();

        if !status.success() {
            let stderr = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = Vec::new();
                use tokio::io::AsyncReadExt;
                let _ = stderr.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            } else {
                String::new()
            };
            return Err(TripleWrapperError::CompressionFailed(stderr));
        }

        stats.bytes_written = bytes_written;
        stats.avg_write_mbps = stats.bytes_written as f64 / stats.duration.as_secs_f64() / 1_048_576.0;

        Ok(stats)
    }

    /// Create/modify archive (add files)
    pub async fn add(
        &self,
        archive: &Path,
        files: &[PathBuf],
        compression_level: u8,
        workdir: Option<&Path>,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<ProgressTelemetry>>,
    ) -> Result<OperationStats> {
        let format = ArchiveFormat::from_extension(&archive.to_path_buf())
            .ok_or_else(|| TripleWrapperError::InvalidFormat("Unknown archive format".into()))?;

        let start = Instant::now();
        let mut stats = OperationStats {
            duration: std::time::Duration::ZERO,
            bytes_read: 0,
            bytes_written: 0,
            bytes_compressed: 0,
            avg_read_mbps: 0.0,
            avg_write_mbps: 0.0,
            avg_compress_mbps: 0.0,
            peak_memory_mb: 0,
            checksum: None,
        };

        let mut cmd = match format {
            ArchiveFormat::SevenZ => {
                let mut cmd = Command::new(&self.sevenz_path);
                cmd.args(["a", "-mx", &compression_level.to_string()]);
                if let Some(wd) = workdir {
                    cmd.arg(format!("-w{}", wd.display()));
                }
                cmd.arg(archive.to_str().unwrap());
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::Zip => {
                let mut cmd = Command::new(&self.sevenz_path);
                cmd.args(["a", "-tzip", "-mx", &compression_level.to_string()]);
                if let Some(wd) = workdir {
                    cmd.arg(format!("-w{}", wd.display()));
                }
                cmd.arg(archive.to_str().unwrap());
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::Tar => {
                let mut cmd = Command::new(&self.tar_path);
                cmd.args(["-cf", archive.to_str().unwrap()]);
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::TarGz => {
                let mut cmd = Command::new(&self.tar_path);
                cmd.args(["-czf", archive.to_str().unwrap()]);
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::TarXz => {
                let mut cmd = Command::new(&self.tar_path);
                cmd.args(["-cJf", archive.to_str().unwrap()]);
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::TarZst => {
                let mut cmd = Command::new(&self.tar_path);
                cmd.args(["-c", "--zstd", "-f", archive.to_str().unwrap()]);
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::TarBz2 => {
                let mut cmd = Command::new(&self.tar_path);
                cmd.args(["-cjf", archive.to_str().unwrap()]);
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            ArchiveFormat::Pixz => {
                // pixz creates .tar.xz, not directly adding to existing
                return Err(TripleWrapperError::InvalidFormat("Pixz doesn't support incremental add".into()));
            }
        };

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("Running: {:?}", cmd);

        let mut child = cmd.spawn()?;

        if let Some(stderr) = child.stderr.take() {
            let mut reader = BufReader::new(stderr).lines();
            while let Some(line) = reader.next_line().await? {
                debug!("7z: {}", line);
                // Parse progress similar to extract
            }
        }

        let status = child.wait().await?;
        stats.duration = start.elapsed();

        if !status.success() {
            let stderr = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = Vec::new();
                use tokio::io::AsyncReadExt;
                let _ = stderr.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            } else {
                String::new()
            };
            return Err(TripleWrapperError::CompressionFailed(stderr));
        }

        Ok(stats)
    }

    /// Delete files from archive
    pub async fn delete(
        &self,
        archive: &Path,
        files: &[String],
        workdir: Option<&Path>,
    ) -> Result<OperationStats> {
        let format = ArchiveFormat::from_extension(&archive.to_path_buf())
            .ok_or_else(|| TripleWrapperError::InvalidFormat("Unknown archive format".into()))?;

        let start = Instant::now();
        let mut stats = OperationStats::default();

        let mut cmd = match format {
            ArchiveFormat::SevenZ | ArchiveFormat::Zip => {
                let mut cmd = Command::new(&self.sevenz_path);
                cmd.args(["d"]);
                if let Some(wd) = workdir {
                    cmd.arg(format!("-w{}", wd.display()));
                }
                cmd.arg(archive.to_str().unwrap());
                for file in files {
                    cmd.arg(file);
                }
                cmd
            }
            _ => return Err(TripleWrapperError::InvalidFormat("Delete only supported for 7z/zip".into())),
        };

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let status = child.wait().await?;
        stats.duration = start.elapsed();

        if !status.success() {
            let stderr = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = Vec::new();
                use tokio::io::AsyncReadExt;
                let _ = stderr.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            } else {
                String::new()
            };
            return Err(TripleWrapperError::CompressionFailed(stderr));
        }

        Ok(stats)
    }

    /// Test archive integrity
    pub async fn test(&self, archive: &Path) -> Result<bool> {
        let format = ArchiveFormat::from_extension(&archive.to_path_buf())
            .ok_or_else(|| TripleWrapperError::InvalidFormat("Unknown archive format".into()))?;

        let archive_str = archive.to_str().unwrap();
        let mut cmd = if matches!(format, ArchiveFormat::SevenZ | ArchiveFormat::Zip) {
            let mut cmd = Command::new(&self.sevenz_path);
            cmd.args(["t", archive_str]);
            cmd
        } else {
            let mut cmd = Command::new(&self.tar_path);
            cmd.args(["-tf", archive_str]);
            cmd
        };

        cmd.stdout(Stdio::null())
            .stderr(Stdio::null());

        let status = cmd.status().await?;
        Ok(status.success())
    }

    /// Parse 7z progress line for bytes written
    fn parse_7z_progress(line: &str) -> Option<u64> {
        // 7z output examples:
        // "Compressing  file.txt  1234567  45%"
        // "Extracting  file.dat  987654321"
        
        if line.contains("Compressing") || line.contains("Extracting") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for part in parts {
                if let Ok(bytes) = part.parse::<u64>() {
                    if bytes > 1000 { // Heuristic: size in bytes
                        return Some(bytes);
                    }
                }
            }
        }
        None
    }
}

impl Default for ArchiveOperator {
    fn default() -> Self {
        Self::new().expect("Failed to create ArchiveOperator")
    }
}

impl Default for OperationStats {
    fn default() -> Self {
        Self {
            duration: std::time::Duration::ZERO,
            bytes_read: 0,
            bytes_written: 0,
            bytes_compressed: 0,
            avg_read_mbps: 0.0,
            avg_write_mbps: 0.0,
            avg_compress_mbps: 0.0,
            peak_memory_mb: 0,
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_find_tools() {
        let op = ArchiveOperator::new();
        assert!(op.is_ok(), "7z and tar should be available");
    }
}