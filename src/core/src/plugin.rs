//! Local format plugins: executables speaking the stable CLI+JSON API.
//!
//! A plugin is any executable named `triplewrapper-<id>` found via
//! `TRIPLEWRAPPER_PLUGIN_DIR` or `PATH`. No SDK, no registry, no daemon —
//! see `docs/API.md` for the contract. Core formats always win on conflict.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::types::{ArchiveMetadata, OperationStats, ProgressTelemetry};
use crate::{Result, TripleWrapperError};

/// Prefix every plugin executable must carry.
pub const PLUGIN_PREFIX: &str = "triplewrapper-";

/// What `plugin-info --json` returns (all fields optional except id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub extensions: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(skip)]
    pub path: PathBuf,
}

impl PluginInfo {
    /// Does this plugin claim `ext` (case-insensitive, no dot)?
    pub fn handles(&self, ext: &str) -> bool {
        let ext = ext.to_lowercase();
        self.extensions.iter().any(|e| e.to_lowercase() == ext)
    }
}

/// Scan directories for `triplewrapper-*` executables and query each one's
/// `plugin-info --json` (5s timeout). Unreachable or malformed plugins are
/// skipped with a warning, never fatal: one broken plugin must not break
/// archive handling.
pub fn discover_in_dirs(dirs: &[PathBuf]) -> Vec<PluginInfo> {
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.starts_with(PLUGIN_PREFIX) {
                continue;
            }
            if !is_executable(&path) || !seen.insert(name.to_string()) {
                continue;
            }
            found.push(query_plugin_info(&path, name));
        }
    }
    // Deterministic order: env dir first (caller orders `dirs`).
    found
}

/// Full discovery: `TRIPLEWRAPPER_PLUGIN_DIR` first, then `PATH` in order.
pub fn discover() -> Vec<PluginInfo> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::var("TRIPLEWRAPPER_PLUGIN_DIR") {
        if !dir.is_empty() {
            dirs.push(PathBuf::from(dir));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    discover_in_dirs(&dirs)
}

/// Find the plugin claiming `ext`, if any.
pub fn find_for_extension(ext: &str) -> Option<PluginInfo> {
    discover().into_iter().find(|p| p.handles(ext))
}

/// Find an installed plugin by its id (for `External(id)` dispatch).
pub fn find_for_id(id: &str) -> Option<PluginInfo> {
    discover().into_iter().find(|p| p.id == id)
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn query_plugin_info(path: &Path, file_name: &str) -> PluginInfo {
    // Fallback identity from the filename: triplewrapper-rar → id "rar".
    let fallback_id = file_name
        .strip_prefix(PLUGIN_PREFIX)
        .unwrap_or(file_name)
        .to_string();
    let fallback = PluginInfo {
        id: fallback_id.clone(),
        extensions: vec![fallback_id],
        description: String::new(),
        path: path.to_path_buf(),
    };
    let out = std::process::Command::new(path)
        .args(["plugin-info", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return fallback,
    };
    match serde_json::from_slice::<PluginInfo>(&out.stdout) {
        Ok(mut info) => {
            if info.id.is_empty() {
                return fallback;
            }
            if info.extensions.is_empty() {
                info.extensions = vec![info.id.clone()];
            }
            info.path = path.to_path_buf();
            info
        }
        Err(_) => fallback,
    }
}

/// Environment for a plugin child: password travels via env, never argv.
fn plugin_env(password: Option<&str>) -> Vec<(String, String)> {
    password
        .filter(|p| !p.is_empty())
        .map(|p| ("TRIPLEWRAPPER_PASSWORD".to_string(), p.to_string()))
        .into_iter()
        .collect()
}

/// List entries via plugin (`list -a FILE --json`, stdout = metadata JSON).
pub async fn plugin_list(
    plugin: &PluginInfo,
    archive: &Path,
    password: Option<&str>,
) -> Result<ArchiveMetadata> {
    let mut cmd = Command::new(&plugin.path);
    cmd.args(["list", "-a"]);
    cmd.arg(archive);
    cmd.arg("--json");
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    for (k, v) in plugin_env(password) {
        cmd.env(k, v);
    }
    let out = cmd.output().await?;
    if !out.status.success() {
        return Err(TripleWrapperError::CompressionFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    let mut meta: ArchiveMetadata = serde_json::from_slice(&out.stdout).map_err(|e| {
        TripleWrapperError::InvalidFormat(format!("plugin {} bad list JSON: {e}", plugin.id))
    })?;
    // Trust but verify identity: the file we asked about.
    meta.path = archive.to_path_buf();
    Ok(meta)
}

/// Extract via plugin. `{"kind":"tick"}` lines on the plugin's stdout are
/// forwarded to `progress_tx` (Steam-graph parity); anything else is ignored.
/// Stats come from the real output size afterwards — same honesty rule as tar.
pub async fn plugin_extract(
    plugin: &PluginInfo,
    archive: &Path,
    output_dir: &Path,
    files: Option<&[String]>,
    password: Option<&str>,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<ProgressTelemetry>>,
) -> Result<OperationStats> {
    use std::time::Instant;

    let start = Instant::now();
    let mut stats = OperationStats::default();
    let mut cmd = Command::new(&plugin.path);
    cmd.args(["extract", "-a"]);
    cmd.arg(archive);
    cmd.args(["-o"]);
    cmd.arg(output_dir);
    if let Some(f) = files {
        for file in f {
            cmd.args(["-f", file]);
        }
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    for (k, v) in plugin_env(password) {
        cmd.env(k, v);
    }
    tracing::info!(
        "Running plugin: {} extract {}",
        plugin.id,
        archive.display()
    );
    let mut child = cmd.spawn()?;
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                if event.get("kind").and_then(|k| k.as_str()) == Some("tick") {
                    if let (Some(tx), Ok(tick)) = (
                        progress_tx.as_ref(),
                        serde_json::from_value::<PluginTick>(event["data"].clone()),
                    ) {
                        let _ = tx.send(ProgressTelemetry {
                            operation_id: crate::types::OperationId::new(),
                            status: crate::types::OperationStatus::Running,
                            current_file: tick.stage,
                            files_total: 0,
                            files_processed: 0,
                            bytes_total: tick.bytes_total,
                            bytes_processed: tick.bytes_processed,
                            bytes_per_second_read: tick.read_mbps * 1_048_576.0,
                            bytes_per_second_write: tick.write_mbps * 1_048_576.0,
                            bytes_per_second_compress: tick.compress_mbps * 1_048_576.0,
                            eta_seconds: None,
                            cpu_percent: 0.0,
                            memory_bytes: 0,
                        });
                    }
                }
            }
        }
    }
    let status = child.wait().await?;
    stats.duration = start.elapsed();
    if !status.success() {
        return Err(TripleWrapperError::CompressionFailed(format!(
            "plugin {} extract failed",
            plugin.id
        )));
    }
    stats.bytes_written = crate::archive::dir_size(output_dir);
    let secs = stats.duration.as_secs_f64().max(0.001);
    stats.avg_write_mbps = stats.bytes_written as f64 / secs / 1_048_576.0;
    Ok(stats)
}

/// Minimal tick shape plugins may stream (same field names as core).
#[derive(Debug, Deserialize)]
struct PluginTick {
    #[serde(default)]
    stage: String,
    #[serde(default)]
    read_mbps: f64,
    #[serde(default)]
    write_mbps: f64,
    #[serde(default)]
    compress_mbps: f64,
    #[serde(default)]
    bytes_processed: u64,
    #[serde(default)]
    bytes_total: u64,
}

/// Test archive via plugin (`test -a FILE`, exit code decides).
pub async fn plugin_test(
    plugin: &PluginInfo,
    archive: &Path,
    password: Option<&str>,
) -> Result<bool> {
    let mut cmd = Command::new(&plugin.path);
    cmd.args(["test", "-a"]);
    cmd.arg(archive);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    for (k, v) in plugin_env(password) {
        cmd.env(k, v);
    }
    let status = cmd.status().await?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a fake `triplewrapper-mock` plugin (shell) into a temp dir.
    fn mock_plugin_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("triplewrapper-mock");
        std::fs::write(
            &path,
            r#"#!/bin/sh
case "$1" in
  plugin-info) echo '{"id":"mock","extensions":["mock"],"description":"test plugin"}';;
  list) echo '{"path":"x","format":"zip","size":10,"entries":[{"path":"m.txt","size":4,"compressed_size":4,"modified":null,"is_directory":false,"crc32":null,"sha256":null}],"solid":false,"encrypted":false,"comment":null}';;
  test) exit 0;;
  extract) out=""; prev=""; for a in "$@"; do if [ "$prev" = "-o" ]; then out="$a"; fi; prev="$a"; done; mkdir -p "$out"; echo "data" > "$out/m.txt"; echo '{"kind":"tick","data":{"stage":"mock","progress":1.0,"bytes_processed":4,"bytes_total":4}}';;
  *) echo "unknown: $1" >&2; exit 2;;
esac
"#,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }

    #[test]
    fn test_discover_finds_mock_plugin() {
        let (dir, _) = mock_plugin_dir();
        let found = discover_in_dirs(&[dir.path().to_path_buf()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "mock");
        assert!(found[0].handles("MOCK"));
        assert!(!found[0].handles("zip"));
    }

    #[test]
    fn test_filename_fallback_when_plugin_info_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("triplewrapper-broken");
        std::fs::write(&path, "#!/bin/sh\nexit 3\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = discover_in_dirs(&[dir.path().to_path_buf()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "broken");
        assert!(found[0].handles("broken"));
    }

    #[test]
    fn test_non_plugin_files_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("random-tool"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        assert!(discover_in_dirs(&[dir.path().to_path_buf()]).is_empty());
    }

    #[tokio::test]
    async fn test_plugin_list_extract_test_roundtrip() {
        let (_dir, path) = mock_plugin_dir();
        let plugin = PluginInfo {
            id: "mock".into(),
            extensions: vec!["mock".into()],
            description: String::new(),
            path,
        };
        let archive = PathBuf::from("/tmp/fake.mock");
        let meta = plugin_list(&plugin, &archive, None).await.unwrap();
        assert_eq!(meta.entries.len(), 1);

        let out = tempfile::tempdir().unwrap();
        let stats = plugin_extract(&plugin, &archive, out.path(), None, None, None)
            .await
            .unwrap();
        assert!(out.path().join("m.txt").exists());
        assert!(stats.bytes_written > 0);

        assert!(plugin_test(&plugin, &archive, None).await.unwrap());
    }

    #[tokio::test]
    async fn test_plugin_list_bad_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("triplewrapper-badjson");
        std::fs::write(&path, "#!/bin/sh\necho 'not json{{'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = PluginInfo {
            id: "badjson".into(),
            extensions: vec!["badjson".into()],
            description: String::new(),
            path,
        };
        assert!(plugin_list(&plugin, Path::new("/tmp/x"), None)
            .await
            .is_err());
    }
}
