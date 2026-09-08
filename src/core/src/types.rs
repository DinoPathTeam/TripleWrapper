//! Core data types for TripleWrapper

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use zvariant_derive::Type;

static CPU_COUNT: OnceLock<usize> = OnceLock::new();

pub fn get_cpu_count() -> usize {
    *CPU_COUNT.get_or_init(|| num_cpus::get())
}

/// Unique operation identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub u64);

impl OperationId {
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported archive formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    SevenZ,
    Zip,
    Tar,
    TarGz,
    TarXz,
    TarZst,
    TarBz2,
    Pixz,
}

impl ArchiveFormat {
    pub fn from_extension(path: &PathBuf) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "7z" => Some(Self::SevenZ),
            "zip" => Some(Self::Zip),
            "tar" => Some(Self::Tar),
            "gz" | "tgz" => Some(Self::TarGz),
            "xz" | "txz" => Some(Self::TarXz),
            "zst" | "tzst" => Some(Self::TarZst),
            "bz2" | "tbz2" => Some(Self::TarBz2),
            "pixz" => Some(Self::Pixz),
            _ => None,
        }
    }

    pub fn default_extension(&self) -> &'static str {
        match self {
            Self::SevenZ => "7z",
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarXz => "tar.xz",
            Self::TarZst => "tar.zst",
            Self::TarBz2 => "tar.bz2",
            Self::Pixz => "tar.pixz",
        }
    }

    pub fn compression_tool(&self) -> &'static str {
        match self {
            Self::SevenZ => "7z",
            Self::Zip => "7z",
            Self::Tar => "tar",
            Self::TarGz => "tar",
            Self::TarXz => "tar",
            Self::TarZst => "tar",
            Self::TarBz2 => "tar",
            Self::Pixz => "pixz",
        }
    }
}

/// Disk/volume information
#[derive(Debug, Clone, Serialize, Deserialize, zvariant::Type)]
pub struct DiskInfo {
    pub mount_point: PathBuf,
    pub label: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub is_removable: bool,
    pub is_system: bool,
    pub device_path: String, // /dev/sdX (empty if unknown)
}

impl DiskInfo {
    pub fn free_gb(&self) -> f64 {
        self.free_bytes as f64 / 1_073_741_824.0
    }

    pub fn total_gb(&self) -> f64 {
        self.total_bytes as f64 / 1_073_741_824.0
    }

    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f64 / self.total_bytes as f64 * 100.0
        }
    }

    pub fn has_space(&self, needed: u64, margin: u64) -> bool {
        self.free_bytes >= needed.saturating_add(margin)
    }
}

impl Default for DiskInfo {
    fn default() -> Self {
        Self {
            mount_point: PathBuf::new(),
            label: String::new(),
            filesystem: String::new(),
            total_bytes: 0,
            free_bytes: 0,
            used_bytes: 0,
            is_removable: false,
            is_system: false,
            device_path: String::new(),
        }
    }
}

/// Compression estimation
#[derive(Debug, Clone, Serialize, Deserialize, zvariant::Type)]
pub struct CompressionEstimate {
    pub current_size: u64,
    pub bytes_to_remove: u64,
    pub bytes_to_add: u64,
    pub compression_ratio: f32, // 0.0 - 1.0
    pub estimated_final_size: u64,
    pub space_needed_for_rewrite: u64,
}

impl CompressionEstimate {
    pub fn new(current_size: u64, bytes_to_remove: u64, bytes_to_add: u64, compression_ratio: f32) -> Self {
        let raw_added = (bytes_to_add as f64 * compression_ratio as f64) as u64;
        let estimated_final = current_size.saturating_sub(bytes_to_remove).saturating_add(raw_added);
        let space_needed = current_size.saturating_add(estimated_final); // In-place safe: original + new

        Self {
            current_size,
            bytes_to_remove,
            bytes_to_add,
            compression_ratio,
            estimated_final_size: estimated_final,
            space_needed_for_rewrite: space_needed,
        }
    }
}

/// Storage decision verdict
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum StorageVerdict {
    InternalOk {
        source_disk: DiskInfo,
        estimate: CompressionEstimate,
        message: String,
    },
    ExternalRequired {
        source_disk: DiskInfo,
        workspace_disk: DiskInfo,
        estimate: CompressionEstimate,
        workdir: PathBuf,
        sevenzip_workdir_param: String, // "-w/path"
        message: String,
        requires_confirmation: bool,
    },
    CriticalError {
        source_disk: DiskInfo,
        estimate: CompressionEstimate,
        best_available_gb: f64,
        message: String,
    },
}

/// Operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationType {
    Extract,
    Modify,  // delete + add
    Clean,   // delete only
    Test,    // integrity check
    List,    // list contents
}

/// Operation status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Pending,
    Preparing,
    Running,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

/// Real-time progress telemetry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressTelemetry {
    pub operation_id: OperationId,
    pub status: OperationStatus,
    pub current_file: String,
    pub files_total: u32,
    pub files_processed: u32,
    pub bytes_total: u64,
    pub bytes_processed: u64,
    pub bytes_per_second_read: f64,
    pub bytes_per_second_write: f64,
    pub bytes_per_second_compress: f64,
    pub eta_seconds: Option<u64>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// Archive entry (file inside archive)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub path: String,
    pub size: u64,
    pub compressed_size: u64,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    pub is_directory: bool,
    pub crc32: Option<u32>,
    pub sha256: Option<String>,
}

/// Archive metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveMetadata {
    pub path: PathBuf,
    pub format: ArchiveFormat,
    pub size: u64,
    pub entries: Vec<ArchiveEntry>,
    pub solid: bool,
    pub encrypted: bool,
    pub comment: Option<String>,
}

/// Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_compression_level: u8,      // 1-9
    pub default_compression_ratio: f32,     // for estimation
    pub safety_margin_bytes: u64,           // extra space required
    pub cache_directories: Vec<PathBuf>,    // preferred cache locations
    pub max_parallel_jobs: usize,
    pub verify_checksums: bool,
    pub checksum_algorithm: ChecksumAlgorithm,
    pub auto_select_workspace: bool,
    pub keep_temp_on_failure: bool,
    pub ui_theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_compression_level: 5,
            default_compression_ratio: 0.5,
            safety_margin_bytes: 512 * 1024 * 1024, // 512 MB
            cache_directories: vec![
                PathBuf::from("/mnt"),
                PathBuf::from("/media"),
                PathBuf::from("/run/media"),
            ],
            max_parallel_jobs: get_cpu_count(),
            verify_checksums: true,
            checksum_algorithm: ChecksumAlgorithm::Blake3,
            auto_select_workspace: true,
            keep_temp_on_failure: false,
            ui_theme: "auto".to_string(),
        }
    }
}

/// Checksum algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgorithm {
    Blake3,
    Sha256,
    Xxh3,
}

impl ChecksumAlgorithm {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Blake3 => "BLAKE3",
            Self::Sha256 => "SHA-256",
            Self::Xxh3 => "XXH3",
        }
    }
}

/// Operation request (IPC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRequest {
    pub id: OperationId,
    pub op_type: OperationType,
    pub archive_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub files_to_process: Vec<String>, // empty = all
    pub compression_level: u8,
    pub compression_format: Option<ArchiveFormat>,
    pub workspace_override: Option<PathBuf>,
    pub verify_after: bool,
    pub dry_run: bool,
}

/// Operation response (IPC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    pub id: OperationId,
    pub success: bool,
    pub message: String,
    pub verdict: Option<StorageVerdict>,
    pub output_path: Option<PathBuf>,
    pub stats: Option<OperationStats>,
}

/// Operation statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    pub duration: Duration,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub bytes_compressed: u64,
    pub avg_read_mbps: f64,
    pub avg_write_mbps: f64,
    pub avg_compress_mbps: f64,
    pub peak_memory_mb: u64,
    pub checksum: Option<String>,
}