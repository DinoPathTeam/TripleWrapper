//! External device mounting via UDisks2 (`udisksctl`) + enumeration via `lsblk`.
//!
//! Design notes (local-only):
//! - We never mount anything ourselves: `udisksctl` talks to the UDisks2
//!   daemon, which owns authentication (polkit prompt when needed) and
//!   applies safe default mount options (nosuid, nodev on removable media).
//! - The GUI only ever offers devices WE enumerated as unmounted partitions;
//!   `mount()` refuses anything else, so no CLI-arg injection is possible.
//! - Enumeration is read-only (`lsblk -J`); no D-Bus proxy code needed.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{Result, TripleWrapperError};

/// A partition that exists but is not mounted anywhere.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmountedDevice {
    /// e.g. `/dev/sdb1`. Always taken from our own `lsblk` enumeration.
    pub dev_path: String,
    pub label: String,
    pub fstype: String,
    pub size_bytes: u64,
    pub removable: bool,
}

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    #[serde(default)]
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    rm: Option<bool>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

/// Parse `lsblk -J` JSON into unmounted partitions (pure, unit-tested).
fn parse_unmounted(json: &str) -> Vec<UnmountedDevice> {
    let mut out = Vec::new();
    let parsed: LsblkOutput = match serde_json::from_str(json) {
        Ok(p) => p,
        Err(_) => return out,
    };
    let mut stack: Vec<&LsblkDevice> = parsed.blockdevices.iter().collect();
    while let Some(dev) = stack.pop() {
        stack.extend(dev.children.iter());
        // Only partitions with a filesystem, currently nowhere mounted.
        if dev.kind.as_deref() != Some("part") {
            continue;
        }
        let fstype = dev.fstype.clone().unwrap_or_default();
        if fstype.is_empty() || fstype == "swap" {
            continue;
        }
        if dev.mountpoint.as_ref().is_some_and(|m| !m.is_empty()) {
            continue;
        }
        let Some(path) = dev.path.clone() else {
            continue;
        };
        if !is_device_path(&path) {
            continue;
        }
        out.push(UnmountedDevice {
            dev_path: path,
            label: dev
                .path
                .as_deref()
                .and_then(|p| p.rsplit('/').next())
                .unwrap_or("disco")
                .to_string(),
            fstype,
            size_bytes: dev.size.unwrap_or(0),
            removable: dev.rm.unwrap_or(false),
        });
    }
    // Removable first, then biggest — the drive you just plugged in wins.
    out.sort_by(|a, b| {
        b.removable
            .cmp(&a.removable)
            .then_with(|| b.size_bytes.cmp(&a.size_bytes))
    });
    out
}

/// Strict device-path shape check (`/dev/...`, safe charset only).
fn is_device_path(path: &str) -> bool {
    if !path.starts_with("/dev/") || path.len() > 64 {
        return false;
    }
    let rest = &path[5..];
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

/// List unmounted partitions via `lsblk -J`.
pub fn list_unmounted() -> Result<Vec<UnmountedDevice>> {
    let which = Command::new("which")
        .arg("lsblk")
        .output()
        .map_err(|_| TripleWrapperError::ToolNotFound("lsblk".into()))?;
    if !which.status.success() {
        return Err(TripleWrapperError::ToolNotFound("lsblk".into()));
    }
    let out = Command::new("lsblk")
        .args(["-J", "-o", "NAME,PATH,MOUNTPOINT,FSTYPE,SIZE,TYPE,RM"])
        .stdin(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_unmounted(&String::from_utf8_lossy(&out.stdout)))
}

/// Mount `dev_path` via UDisks2. Returns the mount point.
///
/// Safety: `dev_path` must come from [`list_unmounted`] — anything else is
/// refused, so callers (CLI/GUI) cannot be tricked into mounting arbitrary
/// paths. Authentication (if the device needs it) is handled by polkit with
/// the desktop's own prompt; we never see credentials.
pub fn mount(dev_path: &str) -> Result<PathBuf> {
    let candidates = list_unmounted()?;
    let dev = candidates
        .iter()
        .find(|d| d.dev_path == dev_path)
        .ok_or_else(|| {
            TripleWrapperError::Internal(format!(
                "device not in unmounted list (already mounted or unknown): {dev_path}"
            ))
        })?;

    let out = Command::new("udisksctl")
        .args(["mount", "-b", &dev.dev_path, "--no-user-interaction"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| TripleWrapperError::ToolNotFound("udisksctl".into()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    // `Mounted /dev/sdb1 at /run/media/user/Seagate.`
    if let Some(at) = stdout.split(" at ").nth(1) {
        let path = at.trim().trim_end_matches('.').to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(TripleWrapperError::Internal(if stderr.is_empty() {
        stdout.trim().to_string()
    } else {
        stderr
    }))
}

/// Unmount `dev_path` via UDisks2.
pub fn unmount(dev_path: &str) -> Result<()> {
    if !is_device_path(dev_path) {
        return Err(TripleWrapperError::Internal(format!(
            "refusing to unmount suspicious path: {dev_path}"
        )));
    }
    let out = Command::new("udisksctl")
        .args(["unmount", "-b", dev_path, "--no-user-interaction"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| TripleWrapperError::ToolNotFound("udisksctl".into()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(TripleWrapperError::Internal(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Make a workspace dir usable: create it and prove writability with a
/// temp file (write + fsync + delete). Catches read-only mounts and
/// permission problems BEFORE a multi-GB operation starts.
pub fn ensure_workspace_ready(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    let probe = path.join(".triplewrapper-write-test");
    match std::fs::write(&probe, b"tw") {
        Ok(()) => {
            // Best effort: make sure it really hit the disk.
            if let Ok(f) = std::fs::File::open(&probe) {
                let _ = f.sync_all();
            }
            let _ = std::fs::remove_file(&probe);
            Ok(path.to_path_buf())
        }
        Err(e) => Err(TripleWrapperError::Internal(format!(
            "workspace not writable ({}): {}",
            path.display(),
            e
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSBLK_FIXTURE: &str = r#"{
        "blockdevices": [
            {"name": "sda", "path": "/dev/sda", "mountpoint": null,
             "fstype": null, "size": 500107862016, "type": "disk", "rm": false,
             "children": [
                {"name": "sda1", "path": "/dev/sda1", "mountpoint": "/",
                 "fstype": "btrfs", "size": 499000000000, "type": "part", "rm": false}
             ]},
            {"name": "sdb", "path": "/dev/sdb", "mountpoint": null,
             "fstype": null, "size": 500107862016, "type": "disk", "rm": true,
             "children": [
                {"name": "sdb1", "path": "/dev/sdb1", "mountpoint": null,
                 "fstype": "exfat", "size": 499000000000, "type": "part", "rm": true},
                {"name": "sdb2", "path": "/dev/sdb2", "mountpoint": null,
                 "fstype": "swap", "size": 1000000, "type": "part", "rm": true}
             ]},
            {"name": "sdc", "path": "/dev/sdc", "mountpoint": null,
             "fstype": null, "size": 8000000, "type": "disk", "rm": true,
             "children": [
                {"name": "sdc1", "path": "/dev/sdc1", "mountpoint": null,
                 "fstype": null, "size": 7000000, "type": "part", "rm": true}
             ]}
        ]
    }"#;

    #[test]
    fn test_parse_unmounted_skips_mounted_swap_and_unformatted() {
        let devs = parse_unmounted(LSBLK_FIXTURE);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].dev_path, "/dev/sdb1");
        assert_eq!(devs[0].fstype, "exfat");
        assert!(devs[0].removable);
    }

    #[test]
    fn test_parse_unmounted_invalid_json_is_empty() {
        assert!(parse_unmounted("not json{{").is_empty());
    }

    #[test]
    fn test_is_device_path_rejects_injection() {
        assert!(is_device_path("/dev/sdb1"));
        assert!(is_device_path("/dev/nvme0n1p2"));
        assert!(!is_device_path("/dev/sdb1; rm -rf /"));
        assert!(!is_device_path("/etc/passwd"));
        assert!(!is_device_path("sdb1"));
        assert!(!is_device_path("/dev/"));
    }

    #[test]
    fn test_mount_refuses_unknown_device() {
        // Must fail without touching udisksctl: device not enumerated.
        // (Uses real lsblk; on machines without it, ToolNotFound is also fine.)
        match mount("/dev/does-not-exist-xyz") {
            Err(TripleWrapperError::Internal(_)) | Err(TripleWrapperError::ToolNotFound(_)) => {}
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    #[test]
    fn test_ensure_workspace_ready_creates_and_proves() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("sub").join(".triplewrapper_cache");
        let out = ensure_workspace_ready(&ws).unwrap();
        assert_eq!(out, ws);
        assert!(ws.is_dir());
        assert!(!ws.join(".triplewrapper-write-test").exists());
    }

    #[test]
    fn test_unmount_refuses_suspicious_path() {
        assert!(unmount("/etc/passwd").is_err());
        assert!(unmount("sdb1").is_err());
    }
}
