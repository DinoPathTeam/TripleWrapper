//! Storage Decision Engine - Core logic for workspace selection

use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::types::*;
use crate::{Result, TripleWrapperError};

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

/// Pure selection policy, hermetically testable: non-system disks with
/// enough free space, removable first, then most free space.
fn select_best_disk(disks: &[DiskInfo], needed: u64) -> Option<DiskInfo> {
    let mut candidates: Vec<&DiskInfo> = disks
        .iter()
        .filter(|d| !d.is_system && d.free_bytes >= needed)
        .collect();
    candidates.sort_by(|a, b| {
        (!a.is_removable)
            .cmp(&!b.is_removable)
            .then_with(|| b.free_bytes.cmp(&a.free_bytes))
    });
    candidates.into_iter().next().cloned()
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
                Se requieren: {:.1} GB para operación segura\n\
                Recomendación: libera espacio en '{}' o conecta una unidad \
                externa con al menos {:.1} GB libres y úsala como destino",
                source_disk.label,
                free_space as f64 / 1e9,
                best_free as f64 / 1e9,
                space_needed as f64 / 1e9,
                source_disk.label,
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

    /// Best non-system disk holding at least `needed` bytes free, if any.
    /// Used to suggest an alternative destination when space runs out.
    /// Returns `None` on machines with no eligible disk (e.g. minimal
    /// containers/VMs where everything is system-mounted) — callers must
    /// fall back to the cleanup tip instead of unwrapping.
    pub fn best_disk_with_space(&mut self, needed: u64) -> Option<DiskInfo> {
        let disks = self.get_disks_owned();
        select_best_disk(&disks, needed)
    }

    /// Preflight for extractions: ensures the OUTPUT disk can hold the
    /// decompressed total plus a safety margin. Pure dry-run — writes
    /// nothing. On failure returns `NoWorkspace` with the full user-facing
    /// guidance (free/needed figures + concrete alternative or cleanup tip).
    ///
    /// Free space is measured with statvfs directly on `output_dir`, NOT
    /// from the disk inventory: virtual filesystems (tmpfs, nfs, fuse…)
    /// are invisible to sysinfo, and falling back to `/` would approve
    /// operations doomed to die halfway (fail-open). Unknown paths fail
    /// closed instead.
    pub fn check_extract_space(
        &mut self,
        output_dir: &Path,
        total_uncompressed: u64,
    ) -> Result<()> {
        use crate::disk::disk_usage;

        let needed = total_uncompressed.saturating_add(self.config.safety_margin_bytes);
        let disks = self.get_disks_owned();
        // A bare "/" match only means "no specific entry" (e.g. tmpfs mounts
        // invisible to the inventory): show the real path instead of a label.
        let label = match self.disk_scanner.find_disk_for_path(output_dir, &disks) {
            Some(d) if d.mount_point != Path::new("/") => d.label.clone(),
            _ => output_dir.display().to_string(),
        };
        let free_bytes = match disk_usage(output_dir) {
            Ok((_, free)) => free,
            Err(e) => {
                return Err(TripleWrapperError::NoWorkspace(format!(
                    "❌ No se pudo medir el espacio de '{}': {}. \
                    Recomendación: verifica que la ruta exista y sea escribible",
                    output_dir.display(),
                    e
                )));
            }
        };
        if free_bytes >= needed {
            return Ok(());
        }
        let suggestion = match self.best_disk_with_space(needed) {
            Some(best) => format!(
                "vuelve a intentarlo usando como destino otra ubicación con espacio \
                (p. ej. {} con {:.1} GB libres)",
                best.mount_point.display(),
                best.free_gb()
            ),
            None => format!(
                "ninguna unidad tiene espacio suficiente: libera al menos {:.1} GB",
                needed as f64 / 1e9
            ),
        };
        Err(TripleWrapperError::NoWorkspace(format!(
            "❌ Espacio insuficiente para esta operación.\n\
            Destino '{}': {:.1} GB libres, se necesitan {:.1} GB.\n\
            Recomendación: libera espacio en '{}' o {}",
            label,
            free_bytes as f64 / 1e9,
            needed as f64 / 1e9,
            label,
            suggestion
        )))
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
    fn test_critical_verdict_carries_recommendation() {
        let mut engine = StorageEngine::new(EngineConfig::default());
        let dir = tempdir().unwrap();
        let file = dir.path().join("huge.zip");
        fs::write(&file, vec![0u8; 100]).unwrap();

        // Absurd demand: no disk on earth complies.
        let est = CompressionEstimate::new(100, 0, u64::MAX / 2, 1.0);
        match engine.decide_workspace(&file, &est) {
            StorageVerdict::CriticalError { message, .. } => {
                assert!(
                    message.contains("Recomendación"),
                    "critical verdict must guide the user, got: {message}"
                );
            }
            other => panic!("Expected CriticalError, got {other:?}"),
        }
    }

    #[test]
    fn test_check_extract_space_ok_and_guided_abort() {
        let mut engine = StorageEngine::new(EngineConfig {
            safety_margin_bytes: 0,
            ..Default::default()
        });
        let dir = tempdir().unwrap();

        // Tiny demand on a real disk: passes.
        engine.check_extract_space(dir.path(), 1024).unwrap();

        // Impossible demand: aborts with actionable guidance, not a bare error.
        let err = engine
            .check_extract_space(dir.path(), u64::MAX)
            .unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("NoWorkspace"), "got: {msg}");
        let text = err.to_string();
        assert!(text.contains("Recomendación"), "got: {text}");
    }

    fn fake_disk(label: &str, free: u64, removable: bool, system: bool) -> DiskInfo {
        DiskInfo {
            mount_point: PathBuf::from(format!("/mnt/{label}")),
            label: label.into(),
            filesystem: "ext4".into(),
            total_bytes: free * 2,
            free_bytes: free,
            used_bytes: free,
            is_removable: removable,
            is_system: system,
            device_path: String::new(),
        }
    }

    #[test]
    fn test_select_best_disk_prefers_removable_then_biggest() {
        // NOTE: fabricated disks on purpose — real runners (containers,
        // minimal VMs) may have no eligible disk at all, so environment
        // hardware must never decide a unit test (see CI failure where
        // best_disk_with_space(1) was None on a bare runner).
        let disks = vec![
            fake_disk("sys", 1_000_000, false, true),
            fake_disk("big-internal", 900_000, false, false),
            fake_disk("small-usb", 100_000, true, false),
            fake_disk("big-usb", 500_000, true, false),
        ];
        // Removable wins over bigger internal.
        assert_eq!(select_best_disk(&disks, 10_000).unwrap().label, "big-usb");
        // System disks never qualify, however roomy.
        assert_eq!(
            select_best_disk(&[fake_disk("sys", u64::MAX / 2, false, true)], 1).map(|d| d.label),
            None::<String>,
        );
        // Nothing fits the demand.
        assert!(select_best_disk(&disks, u64::MAX).is_none());
        // Empty inventory.
        assert!(select_best_disk(&[], 1).is_none());
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
