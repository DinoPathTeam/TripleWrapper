//! TripleWrapper Core Library
//! 
//! High-performance archive management engine with intelligent storage decisions.

pub mod archive;
pub mod cache;
pub mod checksum;
pub mod compression;
pub mod config;
pub mod disk;
pub mod engine;
pub mod error;
pub mod ipc;
pub mod progress;
pub mod types;

pub use error::{Result, TripleWrapperError};
pub use types::*;