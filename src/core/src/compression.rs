//! Compression utilities and format detection

use crate::types::{ArchiveFormat, ChecksumAlgorithm};
use crate::Result;
use std::path::Path;

/// Detect archive format from file content (magic bytes)
pub fn detect_format(path: &Path) -> Result<ArchiveFormat> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    // 512 bytes: covers all fixed magic headers + tar ustar at offset 257.
    let mut header = [0u8; 512];
    let n = file.read(&mut header).unwrap_or(0);
    let header = &header[..n];

    // 7z: 37 7A BC AF 27 1C
    if header.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Ok(ArchiveFormat::SevenZ);
    }

    // ZIP: PK\x03\x04 or PK\x05\x06 or PK\x07\x08
    if header.starts_with(b"PK\x03\x04")
        || header.starts_with(b"PK\x05\x06")
        || header.starts_with(b"PK\x07\x08")
    {
        return Ok(ArchiveFormat::Zip);
    }

    // GZIP: 1F 8B
    if header.starts_with(&[0x1F, 0x8B]) {
        return Ok(ArchiveFormat::TarGz);
    }

    // XZ: FD 37 7A 58 5A 00
    if header.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        return Ok(ArchiveFormat::TarXz);
    }

    // ZSTD: 28 B5 2F FD
    if header.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Ok(ArchiveFormat::TarZst);
    }

    // BZIP2: 42 5A 68
    if header.starts_with(&[0x42, 0x5A, 0x68]) {
        return Ok(ArchiveFormat::TarBz2);
    }

    // TAR: ustar at offset 257 (needs 262 bytes)
    if header.len() >= 262 && &header[257..262] == b"ustar" {
        return Ok(ArchiveFormat::Tar);
    }

    // PIXZ: same as XZ but with index
    // For now, treat as TarXz

    // Default: try extension
    ArchiveFormat::from_extension(path)
        .ok_or_else(|| crate::TripleWrapperError::InvalidFormat("Unknown archive format".into()))
}

/// Get optimal compression level for format
pub fn optimal_compression_level(format: ArchiveFormat, level: u8) -> u8 {
    match format {
        ArchiveFormat::SevenZ => level.min(9),
        ArchiveFormat::Zip => level.min(9),
        ArchiveFormat::TarGz => level.min(9),
        ArchiveFormat::TarXz => level.min(9),
        ArchiveFormat::TarZst => level.min(22), // zstd goes to 22
        ArchiveFormat::TarBz2 => level.min(9),
        ArchiveFormat::Tar => 0, // No compression
        ArchiveFormat::Pixz => level.min(9),
    }
}

/// Get compression tool arguments for format
pub fn compression_args(format: ArchiveFormat, level: u8, threads: usize) -> Vec<String> {
    match format {
        ArchiveFormat::SevenZ => vec![
            "-mx".to_string(),
            level.to_string(),
            "-mmt".to_string(),
            threads.to_string(),
        ],
        ArchiveFormat::Zip => vec!["-tzip".to_string(), "-mx".to_string(), level.to_string()],
        ArchiveFormat::TarGz => vec!["-czf".to_string()],
        ArchiveFormat::TarXz => vec!["-cJf".to_string()],
        ArchiveFormat::TarZst => vec!["--zstd".to_string(), "-f".to_string()],
        ArchiveFormat::TarBz2 => vec!["-cjf".to_string()],
        ArchiveFormat::Tar => vec!["-cf".to_string()],
        ArchiveFormat::Pixz => vec!["-p".to_string(), threads.to_string()],
    }
}

/// Estimate compression ratio for file type
pub fn estimate_ratio_for_extension(ext: &str) -> f32 {
    match ext.to_lowercase().as_str() {
        // Already compressed
        "zip" | "7z" | "gz" | "xz" | "zst" | "bz2" | "rar" | "iso" | "jpg" | "jpeg" | "png"
        | "gif" | "webp" | "mp3" | "mp4" | "mkv" | "avi" | "mov" | "flac" | "ogg" | "opus"
        | "pdf" | "woff" | "woff2" | "ttf" | "otf" => 0.95,

        // Highly compressible
        "txt" | "log" | "csv" | "json" | "xml" | "sql" | "ini" | "cfg" | "conf" | "py" | "rs"
        | "js" | "ts" | "html" | "css" | "md" | "rst" => 0.15,

        // Code/binaries
        "exe" | "dll" | "so" | "dylib" | "bin" | "dat" | "pak" | "chunk" => 0.45,

        // Documents
        "doc" | "docx" | "odt" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" => 0.60,

        // Default
        _ => 0.50,
    }
}

/// Get recommended checksum algorithm for format
pub fn recommended_checksum(format: ArchiveFormat) -> ChecksumAlgorithm {
    match format {
        ArchiveFormat::SevenZ | ArchiveFormat::Zip => ChecksumAlgorithm::Blake3,
        ArchiveFormat::TarGz
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZst
        | ArchiveFormat::TarBz2 => ChecksumAlgorithm::Blake3,
        ArchiveFormat::Tar => ChecksumAlgorithm::Xxh3, // Fast for uncompressed
        ArchiveFormat::Pixz => ChecksumAlgorithm::Blake3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_detect_7z() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.7z");
        let mut f = File::create(&file).unwrap();
        f.write_all(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]).unwrap();
        f.write_all(&[0; 100]).unwrap();

        assert_eq!(detect_format(&file).unwrap(), ArchiveFormat::SevenZ);
    }

    #[test]
    fn test_detect_zip() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.zip");
        let mut f = File::create(&file).unwrap();
        f.write_all(b"PK\x03\x04").unwrap();
        f.write_all(&[0; 100]).unwrap();

        assert_eq!(detect_format(&file).unwrap(), ArchiveFormat::Zip);
    }

    #[test]
    fn test_detect_gzip() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.tar.gz");
        let mut f = File::create(&file).unwrap();
        f.write_all(&[0x1F, 0x8B]).unwrap();
        f.write_all(&[0; 100]).unwrap();

        assert_eq!(detect_format(&file).unwrap(), ArchiveFormat::TarGz);
    }

    #[test]
    fn test_estimate_ratios() {
        assert_eq!(estimate_ratio_for_extension("txt"), 0.15);
        assert_eq!(estimate_ratio_for_extension("jpg"), 0.95);
        assert_eq!(estimate_ratio_for_extension("pak"), 0.45);
        assert_eq!(estimate_ratio_for_extension("unknown"), 0.50);
    }
}
