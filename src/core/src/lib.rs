//! TripleWrapper Core Library
//!
//! High-performance archive management engine with intelligent storage decisions.

pub mod archive;
pub mod checksum;
pub mod compression;
pub mod config;
pub mod disk;
pub mod engine;
pub mod error;
pub mod integrity;
#[cfg(not(test))]
pub mod ipc;
pub mod mount;
pub mod plugin;
pub mod progress;
pub mod queue;
pub mod types;
pub mod utils;
pub mod watch;

pub use error::{Result, TripleWrapperError};
pub use queue::{OperationQueue, Priority, QueueConfig, QueueItem, QueueItemStatus};
pub use types::*;
