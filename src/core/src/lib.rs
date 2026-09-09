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
#[cfg(not(test))]
pub mod ipc;
pub mod progress;
pub mod queue;
pub mod types;
pub mod utils;

pub use error::{Result, TripleWrapperError};
pub use types::*;
pub use queue::{OperationQueue, QueueConfig, QueueItem, QueueItemStatus, Priority};