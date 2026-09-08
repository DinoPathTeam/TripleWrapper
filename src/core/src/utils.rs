//! Utility functions

use std::path::Path;
use std::time::Duration;
use std::os::unix::ffi::OsStrExt;
use tracing::info;

/// Format bytes as human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

/// Format duration as human-readable string
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

/// Format speed as MB/s
pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_048_576.0)
    } else if bytes_per_sec >= 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// Get available space on path
pub fn get_free_space(path: &Path) -> std::io::Result<u64> {
    #[cfg(target_family = "unix")]
    {
        use libc::{statvfs, c_char};
        let mut statfs: statvfs = unsafe { std::mem::zeroed() };
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        let ret = unsafe { statvfs(c_path.as_ptr(), &mut statfs) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(statfs.f_bavail * statfs.f_frsize as u64)
    }
    #[cfg(not(target_family = "unix"))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "get_free_space only implemented for Unix",
        ))
    }
}

/// Ensure directory exists
pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        info!("Created directory: {}", path.display());
    }
    Ok(())
}

/// Sanitize filename for safe filesystem usage
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

/// Get temp directory for triplewrapper
pub fn get_tw_temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("triplewrapper")
}

/// Calculate ETA from bytes processed
pub fn calculate_eta(bytes_processed: u64, bytes_total: u64, bytes_per_sec: f64) -> Option<Duration> {
    if bytes_per_sec <= 0.0 || bytes_total == 0 {
        return None;
    }
    
    let remaining = bytes_total.saturating_sub(bytes_processed);
    let secs = (remaining as f64 / bytes_per_sec) as u64;
    Some(Duration::from_secs(secs))
}

/// Atomic file replace (for In-Place Safe principle)
pub async fn atomic_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    // Use renameat2 with RENAME_EXCHANGE if available, otherwise rename
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        use std::fs;
        
        // Create temp file in same directory as dst
        let temp = dst.with_extension("tmp.tw");
        fs::rename(src, &temp)?;
        
        // Atomic replace
        #[cfg(target_os = "linux")]
        {
            use libc::{renameat2, AT_FDCWD, RENAME_EXCHANGE};
            let dst_c = std::ffi::CString::new(dst.as_os_str().as_bytes())?;
            let temp_c = std::ffi::CString::new(temp.as_os_str().as_bytes())?;
            
            let ret = unsafe {
                renameat2(AT_FDCWD, temp_c.as_ptr(), AT_FDCWD, dst_c.as_ptr(), RENAME_EXCHANGE)
            };
            
            if ret == 0 {
                // Clean up old file (now at temp)
                let _ = fs::remove_file(&temp);
                return Ok(());
            }
        }
        
        // Fallback: simple rename
        fs::rename(&temp, dst)?;
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        std::fs::rename(src, dst)?;
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.5 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m");
        assert_eq!(format_duration(Duration::from_secs(90061)), "1d 1h");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(500.0), "500 B/s");
        assert_eq!(format_speed(2048.0), "2.0 KB/s");
        assert_eq!(format_speed(2_097_152.0), "2.0 MB/s");
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("bad/name.txt"), "bad_name.txt");
        assert_eq!(sanitize_filename("file:name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file\0name.txt"), "file_name.txt");
    }

    #[test]
    fn test_calculate_eta() {
        assert_eq!(calculate_eta(0, 100, 10.0), Some(Duration::from_secs(10)));
        assert_eq!(calculate_eta(50, 100, 10.0), Some(Duration::from_secs(5)));
        assert_eq!(calculate_eta(100, 100, 10.0), Some(Duration::from_secs(0)));
        assert_eq!(calculate_eta(0, 100, 0.0), None);
    }
}