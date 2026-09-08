//! Configuration management

use std::path::{Path, PathBuf};
use config::{Config, File, Environment};
use serde::{Deserialize, Serialize};
use tracing::info;
use dirs_next::config_dir;

use crate::types::Config as AppConfig;
use crate::{Result, TripleWrapperError};

/// Configuration manager
pub struct ConfigManager {
    config: AppConfig,
    config_path: Option<PathBuf>,
}

impl ConfigManager {
    /// Load configuration from default locations
    pub fn load() -> Result<Self> {
        let config_path = Self::find_config_file();
        let mut builder = Config::builder();

        // Load from file if exists
        if let Some(path) = &config_path {
            builder = builder.add_source(File::from(path.clone()));
        }

        // Load from environment variables (prefix: TW_)
        builder = builder.add_source(Environment::with_prefix("TW").separator("_"));

        let config: AppConfig = builder.build()?.try_deserialize()?;

        Ok(Self {
            config,
            config_path,
        })
    }

    /// Get current configuration
    pub fn get(&self) -> &AppConfig {
        &self.config
    }

    /// Get mutable configuration
    pub fn get_mut(&mut self) -> &mut AppConfig {
        &mut self.config
    }

    /// Save configuration to file
    pub fn save(&mut self) -> Result<()> {
        let path = self.config_path.clone().unwrap_or_else(|| {
            config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("triplewrapper")
                .join("config.toml")
        });

        // Create directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let toml = toml::to_string_pretty(&self.config)
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?;

        std::fs::write(&path, toml)?;
        info!("Configuration saved to {}", path.display());

        Ok(())
    }

    /// Reset to defaults
    pub fn reset(&mut self) {
        self.config = AppConfig::default();
    }

    fn find_config_file() -> Option<PathBuf> {
        // Check XDG config directory
        if let Some(config_dir) = config_dir() {
            let path = config_dir.join("triplewrapper").join("config.toml");
            if path.exists() {
                return Some(path);
            }
        }

        // Check current directory
        let local = PathBuf::from("triplewrapper.toml");
        if local.exists() {
            return Some(local);
        }

        None
    }

    /// Get config file path
    pub fn config_path(&self) -> Option<&PathBuf> {
        self.config_path.as_ref()
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self {
            config: AppConfig::default(),
            config_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let manager = ConfigManager::default();
        let config = manager.get();
        
        assert_eq!(config.default_compression_level, 5);
        assert_eq!(config.default_compression_ratio, 0.5);
        assert!(config.verify_checksums);
    }

    #[test]
    fn test_save_load() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("test_config.toml");
        
        let mut manager = ConfigManager::default();
        manager.get_mut().default_compression_level = 9;
        manager.config_path = Some(config_path.clone());
        
        manager.save().unwrap();
        
        // Reload
        let mut builder = Config::builder();
        builder = builder.add_source(File::from(config_path));
        let loaded: AppConfig = builder.build().unwrap().try_deserialize().unwrap();
        
        assert_eq!(loaded.default_compression_level, 9);
    }
}