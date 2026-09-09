//! Storage Decision Engine - Core logic for workspace selection

use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::types::*;

/// Configuration for the storage engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub safety_margin_bytes: u64,
    pub default_compression_ratio: f32,
    pub preferred_cache_dirs: Vec<PathBuf>,
    pub auto_select_workspace: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            safety_margin_bytes: 512 * 1024 * 1024, // 512 MB
            default_compression_ratio: 0.5,
            preferred_cache_dirs: vec![
                PathBuf::from("/mnt"),
                PathBuf::from("/media"),
                PathBuf::from("/run/media"),
            ],
            auto_select_workspace: true,
        }
    }
}

/// Main storage decision engine
pub struct StorageEngine {
    config: EngineConfig,
    disk_scanner: crate::disk::DiskScanner,
    disks_cache: Vec<DiskInfo>,
}

impl StorageEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            disk_scanner: crate::disk::DiskScanner::new(),
            disks_cache: Vec::new(),
        }
    }

    /// Refresh disk information
    pub fn refresh_disks(&mut self) -> &[DiskInfo] {
        self.disks_cache = self.disk_scanner.scan();
        &self.disks_cache
    }

    /// Get cached disks as owned data (refresh if empty)
    pub fn get_disks_owned(&mut self) -> Vec<DiskInfo> {
        if self.disks_cache.is_empty() {
            self.refresh_disks();
        }
        self.disks_cache.clone()
    }

    /// Get cached disks as reference (refresh if empty)
    pub fn get_disks(&mut self) -> &[DiskInfo] {
        if self.disks_cache.is_empty() {
            self.refresh_disks();
        }
        &self.disks_cache
    }

    /// Calculate compression estimate
    pub fn calculate_estimate(
        &self,
        current_size: u64,
        bytes_to_remove: u64,
        bytes_to_add: u64,
        compression_ratio: Option<f32>,
    ) -> CompressionEstimate {
        let ratio = compression_ratio.unwrap_or(self.config.default_compression_ratio);
        CompressionEstimate::new(current_size, bytes_to_remove, bytes_to_add, ratio)
    }

    /// Decide workspace for an operation
    pub fn decide_workspace(
        &mut self,
        archive_path: &Path,
        estimate: &CompressionEstimate,
    ) -> StorageVerdict {
        // Extract config values first to avoid borrow conflicts
        let safety_margin = self.config.safety_margin_bytes;
        let auto_select = self.config.auto_select_workspace;

        // Get owned copy of disks to avoid borrow conflicts
        let disks = self.get_disks_owned();

        // Find source disk
        let source_disk = self
            .disk_scanner
            .find_disk_for_path(archive_path, &disks)
            .cloned()
            .unwrap_or_else(|| {
                // Fallback: root disk
                disks
                    .iter()
                    .find(|d| d.mount_point == Path::new("/"))
                    .cloned()
                    .unwrap_or_else(|| DiskInfo {
                        mount_point: PathBuf::from("/"),
                        label: "Desconocido".to_string(),
                        filesystem: "unknown".to_string(),
                        total_bytes: 0,
                        free_bytes: 0,
                        used_bytes: 0,
                        is_removable: false,
                        is_system: true,
                        device_path: String::new(),
                    })
            });

        let space_needed = estimate
            .space_needed_for_rewrite
            .saturating_add(safety_margin);
        let free_space = source_disk.free_bytes;

        debug!(
            "Archive: {} bytes, Estimated final: {} bytes, Space needed: {} bytes, Free: {} bytes",
            estimate.current_size, estimate.estimated_final_size, space_needed, free_space
        );

        // Case 1: Enough space on source disk
        if free_space >= space_needed {
            info!(
                "Internal OK: {} GB free on {}",
                free_space as f64 / 1e9,
                source_disk.label
            );
            return StorageVerdict::InternalOk {
                source_disk: source_disk.clone(),
                estimate: estimate.clone(),
                message: format!(
                    "Espacio suficiente en '{}' ({:.1} GB libres / {:.1} GB requeridos)",
                    source_disk.label,
                    free_space as f64 / 1e9,
                    space_needed as f64 / 1e9
                ),
            };
        }

        // Case 2: Find external disk with space
        if auto_select {
            let mut candidates: Vec<&DiskInfo> = disks
                .iter()
                .filter(|d| {
                    d.mount_point != source_disk.mount_point
                        && d.free_bytes >= space_needed
                        && !d.is_system
                })
                .collect();

            // Sort: removable first, then most free space
            candidates.sort_by(|a, b| {
                (!a.is_removable)
                    .cmp(&!b.is_removable)
                    .then_with(|| b.free_bytes.cmp(&a.free_bytes))
            });

            if let Some(workspace) = candidates.first() {
                let workdir = workspace.mount_point.join(".triplewrapper_cache");
                let sevenzip_param = format!("-w{}", workdir.display());

                info!(
                    "External workspace selected: {} ({} GB free)",
                    workspace.label,
                    workspace.free_gb()
                );

                return StorageVerdict::ExternalRequired {
                    source_disk: source_disk.clone(),
                    workspace_disk: (*workspace).clone(),
                    estimate: estimate.clone(),
                    workdir: workdir.clone(),
                    sevenzip_workdir_param: sevenzip_param,
                    message: format!(
                        "⚠ Espacio insuficiente en '{}' ({:.1} GB libres, se necesitan {:.1} GB).\n\
                        ✓ Usando caché en '{}' ({:.1} GB libres en {})",
                        source_disk.label,
                        free_space as f64 / 1e9,
                        space_needed as f64 / 1e9,
                        workspace.label,
                        workspace.free_gb(),
                        workspace.mount_point.display()
                    ),
                    requires_confirmation: true,
                };
            }
        }

        // Case 3: No space anywhere
        let best_free = disks.iter().map(|d| d.free_bytes).max().unwrap_or(0);

        warn!(
            "Critical: No space anywhere. Best: {} GB, Needed: {} GB",
            best_free as f64 / 1e9,
            space_needed as f64 / 1e9
        );

        StorageVerdict::CriticalError {
            source_disk: source_disk.clone(),
            estimate: estimate.clone(),
            best_available_gb: best_free as f64 / 1e9,
            message: format!(
                "❌ ALMACENAMIENTO INSUFICIENTE EN TODO EL SISTEMA\n\
                Disco origen '{}': {:.1} GB libres\n\
                Mejor disco disponible: {:.1} GB libres\n\
                Se requieren: {:.1} GB para operación segura",
                source_disk.label,
                free_space as f64 / 1e9,
                best_free as f64 / 1e9,
                space_needed as f64 / 1e9
            ),
        }
    }

    /// Quick verdict for GUI (one-shot)
    pub fn quick_verdict(
        &mut self,
        archive_path: &Path,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> StorageVerdict {
        let current_size = std::fs::metadata(archive_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let estimate = self.calculate_estimate(current_size, bytes_to_remove, bytes_to_add, None);
        self.decide_workspace(archive_path, &estimate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_estimate_calculation() {
        let engine = StorageEngine::new(EngineConfig::default());
        let est = engine.calculate_estimate(1000, 200, 500, Some(0.5));

        // current - removed + added * ratio = 1000 - 200 + 500*0.5 = 1050
        assert_eq!(est.estimated_final_size, 1050);
        // space needed = current + estimated = 1000 + 1050 = 2050
        assert_eq!(est.space_needed_for_rewrite, 2050);
    }

    #[test]
    fn test_estimate_no_overflow() {
        let engine = StorageEngine::new(EngineConfig::default());
        let est = engine.calculate_estimate(u64::MAX, 0, 0, Some(1.0));
        assert_eq!(est.estimated_final_size, u64::MAX);
        assert_eq!(est.space_needed_for_rewrite, u64::MAX);
    }

    #[test]
    fn test_decide_internal_ok() {
        let mut engine = StorageEngine::new(EngineConfig {
            safety_margin_bytes: 0,
            ..Default::default()
        });

        // Create temp file
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.zip");
        fs::write(&file, vec![0u8; 1000]).unwrap();

        let est = CompressionEstimate::new(1000, 100, 100, 0.5); // needs ~2000
        let verdict = engine.decide_workspace(&file, &est);

        match verdict {
            StorageVerdict::InternalOk { .. } => {}
            other => panic!("Expected InternalOk, got {:?}", other),
        }
    }
}
