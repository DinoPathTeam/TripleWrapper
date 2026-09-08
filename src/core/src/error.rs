//! Error types for TripleWrapper

use thiserror::Error;
use std::path::PathBuf;

#[derive(Error, Debug)]
pub enum TripleWrapperError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Archive not found: {0}")]
    ArchiveNotFound(PathBuf),

    #[error("Insufficient space: need {needed} bytes, have {available} bytes on {device}")]
    InsufficientSpace { needed: u64, available: u64, device: String },

    #[error("No suitable workspace found: {0}")]
    NoWorkspace(String),

    #[error("Compression tool not found: {0}")]
    ToolNotFound(String),

    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Invalid archive format: {0}")]
    InvalidFormat(String),

    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("DBus error: {0}")]
    DBus(#[from] zbus::Error),

    #[error("System info error: {0}")]
    SysInfo(#[from] sysinfo::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, TripleWrapperError>;