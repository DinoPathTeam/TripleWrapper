//! Disk scanning and monitoring

use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use sysinfo::{Disks, System};

use crate::types::DiskInfo;

/// Disk scanner using sysinfo + lsblk for labels
pub struct DiskScanner {
    system: System,
    label_cache: HashMap<String, String>,
}

impl DiskScanner {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        Self {
            system,
            label_cache: HashMap::new(),
        }
    }

    /// Scan all mounted disks with space info
    pub fn scan(&mut self) -> Vec<DiskInfo> {
        self.system.refresh_all();
        let mut disks = Vec::new();

        let disks_info = Disks::new_with_refreshed_list();

        for disk in disks_info.iter() {
            let mount_point = disk.mount_point().to_path_buf();

            // Skip virtual filesystems
            let fs = disk.file_system().to_string_lossy().to_string();
            if Self::is_virtual_fs(&fs) {
                continue;
            }

            let total = disk.total_space();
            let free = disk.available_space();
            let used = total.saturating_sub(free);

            // Get label via lsblk (blocking call, but fast)
            let device_name = Self::extract_device_name(disk.name());
            let label = self.get_label(&device_name, &mount_point);
            let is_removable = Self::is_removable(&device_name);
            let is_system = Self::is_system_mount(&mount_point);

            disks.push(DiskInfo {
                mount_point,
                label,
                filesystem: fs,
                total_bytes: total,
                free_bytes: free,
                used_bytes: used,
                is_removable,
                is_system,
                device_path: device_name.unwrap_or_default(),
            });
        }

        // Also check common external mount points not in sysinfo
        disks.extend(self.scan_external_mounts());

        disks
    }

    /// Get disk label via lsblk
    fn get_label(&mut self, device: &Option<String>, mount_point: &Path) -> String {
        if let Some(dev) = device {
            if let Some(cached) = self.label_cache.get(dev) {
                return cached.clone();
            }

            let output = Command::new("lsblk").args(["-no", "LABEL", dev]).output();

            if let Ok(out) = output {
                let label = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !label.is_empty() {
                    self.label_cache.insert(dev.clone(), label.clone());
                    return label;
                }
            }
        }

        // Fallback: mount point name
        mount_point
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Desconocido")
            .to_string()
    }

    /// Scan common external mount points
    fn scan_external_mounts(&self) -> Vec<DiskInfo> {
        const EXTERNAL_PATHS: &[&str] =
            &["/mnt", "/media", "/run/media", "/mnt/external", "/mnt/usb"];
        let mut disks = Vec::new();

        for base in EXTERNAL_PATHS {
            let base_path = Path::new(base);
            if !base_path.exists() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(base_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !Self::is_mount_point(&path) {
                        continue;
                    }

                    if let Ok(usage) = disk_usage(&path) {
                        let label = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Externo")
                            .to_string();

                        disks.push(DiskInfo {
                            mount_point: path,
                            label,
                            filesystem: "unknown".to_string(),
                            total_bytes: usage.0,
                            free_bytes: usage.1,
                            used_bytes: usage.0.saturating_sub(usage.1),
                            is_removable: true,
                            is_system: false,
                            device_path: String::new(),
                        });
                    }
                }
            }
        }
        disks
    }

    /// Find disk containing a path
    pub fn find_disk_for_path<'a>(
        &self,
        path: &Path,
        disks: &'a [DiskInfo],
    ) -> Option<&'a DiskInfo> {
        let abs_path = path.canonicalize().ok()?;
        disks
            .iter()
            .filter(|d| abs_path.starts_with(&d.mount_point))
            .max_by_key(|d| d.mount_point.components().count())
    }

    fn is_virtual_fs(fs: &str) -> bool {
        matches!(
            fs,
            "proc"
                | "sysfs"
                | "devtmpfs"
                | "devpts"
                | "tmpfs"
                | "cgroup"
                | "cgroup2"
                | "pstore"
                | "bpf"
                | "tracefs"
                | "securityfs"
                | "configfs"
                | "debugfs"
                | "hugetlbfs"
                | "mqueue"
                | "nsfs"
                | "rpc_pipefs"
                | "autofs"
                | "efivarfs"
                | "fuse.lxcfs"
                | "fuse.portal"
                | "overlay"
                | "squashfs"
                | "iso9660"
        )
    }

    fn extract_device_name(name: &std::ffi::OsStr) -> Option<String> {
        name.to_str().map(|s| s.trim_start_matches('/').to_string())
    }

    fn is_removable(device: &Option<String>) -> bool {
        if let Some(dev) = device {
            // Check /sys/block/*/removable
            let block_name = dev.trim_start_matches("/dev/");
            let removable_path = format!("/sys/block/{}/removable", block_name);
            if let Ok(content) = std::fs::read_to_string(&removable_path) {
                return content.trim() == "1";
            }
            // Heuristic: sd* or mmcblk* often removable
            block_name.starts_with("sd") || block_name.starts_with("mmcblk")
        } else {
            false
        }
    }

    fn is_system_mount(mount: &Path) -> bool {
        matches!(
            mount.to_str().unwrap_or(""),
            "/" | "/boot" | "/boot/efi" | "/efi" | "/usr" | "/var" | "/etc"
        )
    }

    fn is_mount_point(path: &Path) -> bool {
        // Check if path is a mount point by comparing device IDs with parent
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(_) => return false,
        };
        let parent_meta = match path.parent().and_then(|p| p.metadata().ok()) {
            Some(m) => m,
            None => return true, // Root path is always a mount point
        };
        meta.dev() != parent_meta.dev()
    }
}

/// Cross-platform disk usage using statvfs
#[cfg(target_family = "unix")]
fn disk_usage(path: &Path) -> std::io::Result<(u64, u64)> {
    use libc::statvfs;
    let mut statfs: statvfs = unsafe { std::mem::zeroed() };
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    let ret = unsafe { statvfs(c_path.as_ptr(), &mut statfs) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let total = statfs.f_blocks * statfs.f_frsize as u64;
    let free = statfs.f_bavail * statfs.f_frsize as u64;
    Ok((total, free))
}

#[cfg(not(target_family = "unix"))]
fn disk_usage(_path: &Path) -> std::io::Result<(u64, u64)> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "disk_usage only implemented for Unix",
    ))
}

impl Default for DiskScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan() {
        let mut scanner = DiskScanner::new();
        let disks = scanner.scan();
        assert!(!disks.is_empty(), "Should find at least root disk");

        for disk in &disks {
            println!(
                "{}: {} GB free / {} GB total",
                disk.label,
                disk.free_gb(),
                disk.total_gb()
            );
        }
    }
}
