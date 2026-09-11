//! TripleWrapper Core - Main binary

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use triplewrapper_core::{
    archive::ArchiveOperator,
    disk::DiskScanner,
    engine::StorageEngine,
    error::TripleWrapperError,
    queue::{OperationQueue, Priority, QueueConfig, QueueItemStatus},
    types::{OperationId, OperationRequest, OperationType, Password, StorageVerdict},
    utils::{format_bytes, format_duration},
    Result,
};

#[derive(Parser)]
#[command(name = "triplewrapper")]
#[command(about = "TripleWrapper - Smart archive manager with intelligent storage decisions")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, global = true, help = "Verbose output")]
    verbose: bool,

    #[arg(short, long, global = true, help = "JSON output")]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan and list disks with free space
    Disks,

    /// Analyze storage requirements for an operation
    Analyze {
        /// Archive file path
        #[arg(short, long)]
        archive: String,

        /// Bytes to remove (uncompressed)
        #[arg(long, default_value = "0")]
        remove: u64,

        /// Bytes to add (uncompressed)
        #[arg(long, default_value = "0")]
        add: u64,

        /// Compression ratio hint (0.0-1.0)
        #[arg(long, default_value = "0.5")]
        ratio: f32,
    },

    /// List archive contents
    List {
        #[arg(short, long)]
        archive: String,

        /// Archive password (prefer TRIPLEWRAPPER_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,
    },

    /// Extract archive
    Extract {
        #[arg(short, long)]
        archive: String,

        #[arg(short, long)]
        output: Option<String>,

        #[arg(short, long)]
        files: Vec<String>,

        /// Archive password (prefer TRIPLEWRAPPER_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,

        /// Resume: skip files already extracted with matching size
        #[arg(long, default_value_t = false)]
        resume: bool,
    },

    /// Test archive integrity
    Test {
        #[arg(short, long)]
        archive: String,

        /// Archive password (prefer TRIPLEWRAPPER_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,
    },

    /// Create an archive from files (format from archive suffix)
    Create {
        /// Output archive path, e.g. backup.7z (suffix selects format)
        #[arg(short, long)]
        archive: String,

        /// Files/dirs to add (repeatable)
        #[arg(short, long)]
        files: Vec<PathBuf>,

        /// Compression level (format-capped)
        #[arg(long, default_value_t = 5)]
        level: u8,

        /// Archive password for 7z/zip (prefer TRIPLEWRAPPER_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,
    },

    /// Delete entries from a 7z/zip archive
    Delete {
        #[arg(short, long)]
        archive: String,

        /// Archive-internal paths to delete (repeatable)
        #[arg(short, long)]
        files: Vec<String>,

        /// Archive password (prefer TRIPLEWRAPPER_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,
    },

    /// Run DBus service
    Serve,

    /// Run a real operation emitting JSON tick events on stdout
    Run {
        /// Archive file path
        #[arg(short, long)]
        archive: String,

        /// Workspace directory (cache / output base)
        #[arg(short, long)]
        workspace: String,

        /// Operation: test | extract
        #[arg(short, long, default_value = "test")]
        operation: String,

        /// Output directory for extract (defaults to workspace)
        #[arg(short, long)]
        output: Option<String>,

        /// Archive password (prefer TRIPLEWRAPPER_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,

        /// Resume: skip files already extracted with matching size
        #[arg(long, default_value_t = false)]
        resume: bool,
    },

    /// List unmounted external devices (USB-HDD/SSD ready to mount)
    Devices,

    /// Mount an unmounted device via UDisks2 (desktop handles auth)
    Mount {
        /// Device path from `devices`, e.g. /dev/sdb1
        #[arg(short, long)]
        device: String,
    },

    /// Unmount a device via UDisks2
    Unmount {
        /// Device path, e.g. /dev/sdb1
        #[arg(short, long)]
        device: String,
    },

    /// Watch UDisks2 for plug/unplug events, streaming JSON lines.
    /// The GUI subscribes to this instead of polling.
    Watch,

    /// Queue management commands
    #[command(subcommand)]
    Queue(QueueCommands),

    /// Local integrity database (BLAKE3 log, no network)
    #[command(subcommand)]
    Integrity(IntegrityCommands),
}

/// Local integrity database subcommands
#[derive(Subcommand)]
enum IntegrityCommands {
    /// Hash an archive with BLAKE3 and append a record
    Record {
        #[arg(short, long)]
        archive: String,

        #[arg(short, long, default_value = "manual")]
        operation: String,
    },

    /// Compare current file against its last record
    Check {
        #[arg(short, long)]
        archive: String,
    },

    /// Show the last record for an archive
    Last {
        #[arg(short, long)]
        archive: String,
    },
}

/// Queue management subcommands
#[derive(Subcommand)]
enum QueueCommands {
    /// Add operation to queue
    Add {
        #[arg(short, long)]
        archive: String,

        #[arg(short, long, default_value = "extract")]
        operation: String,

        #[arg(short, long)]
        output: Option<String>,

        #[arg(long, default_value = "normal")]
        priority: String,

        #[arg(long)]
        files: Vec<String>,

        /// Archive password (not persisted: must be re-supplied after restart)
        #[arg(long)]
        password: Option<String>,
    },

    /// Add multiple operations from a batch file
    Batch {
        #[arg(short, long)]
        file: String,
    },

    /// List queued operations
    List {
        #[arg(long, default_value = "all")]
        status: String,

        #[arg(short, long)]
        json: bool,
    },

    /// Show queue status
    Status,

    /// Pause queue processing
    Pause,

    /// Resume queue processing
    Resume,

    /// Pause a specific item
    PauseItem { id: String },

    /// Resume a specific item
    ResumeItem { id: String },

    /// Cancel an item
    Cancel { id: String },

    /// Retry a failed item
    Retry { id: String },

    /// Remove completed/failed items
    Cleanup {
        #[arg(long, default_value = "86400")]
        max_age_seconds: u64,
    },

    /// Start queue workers
    Start {
        #[arg(long, default_value = "2")]
        workers: usize,
    },

    /// Stop queue workers
    Stop,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Commands::Disks => cmd_disks(cli.json).await,
        Commands::Analyze {
            archive,
            remove,
            add,
            ratio,
        } => cmd_analyze(archive, remove, add, ratio, cli.json).await,
        Commands::List { archive, password } => {
            cmd_list(archive, resolve_password(password), cli.json).await
        }
        Commands::Extract {
            archive,
            output,
            files,
            password,
            resume,
        } => {
            cmd_extract(
                archive,
                output,
                files,
                resolve_password(password),
                resume,
                cli.json,
            )
            .await
        }
        Commands::Test { archive, password } => {
            cmd_test(archive, resolve_password(password), cli.json).await
        }
        Commands::Create {
            archive,
            files,
            level,
            password,
        } => cmd_create(archive, files, level, resolve_password(password), cli.json).await,
        Commands::Delete {
            archive,
            files,
            password,
        } => cmd_delete(archive, files, resolve_password(password), cli.json).await,
        Commands::Serve => cmd_serve().await,
        Commands::Run {
            archive,
            workspace,
            operation,
            output,
            password,
            resume,
        } => {
            cmd_run(
                archive,
                workspace,
                operation,
                output,
                resolve_password(password),
                resume,
            )
            .await
        }
        Commands::Devices => cmd_devices(cli.json).await,
        Commands::Mount { device } => cmd_mount(device, cli.json).await,
        Commands::Unmount { device } => cmd_unmount(device, cli.json).await,
        Commands::Watch => cmd_watch().await,
        Commands::Queue(cmd) => cmd_queue(cmd, cli.json).await,
        Commands::Integrity(cmd) => cmd_integrity(cmd, cli.json).await,
    }
}

/// Resolve the archive password: explicit `--password` flag wins,
/// otherwise `TRIPLEWRAPPER_PASSWORD`. The flag is visible in the process
/// list, so the env var is preferred (warned on stderr when flag is used).
/// The returned secret is never logged and never persisted.
fn resolve_password(flag: Option<String>) -> Option<Password> {
    match flag {
        Some(pw) if !pw.is_empty() => {
            eprintln!(
                "warning: --password is visible to other local users via the process list; prefer TRIPLEWRAPPER_PASSWORD"
            );
            Some(Password::new(pw))
        }
        _ => std::env::var("TRIPLEWRAPPER_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty())
            .map(Password::new),
    }
}

fn password_str(pw: Option<&Password>) -> Option<&str> {
    pw.as_ref().map(|p| p.expose())
}

/// Preflight an extraction against the OUTPUT disk using real
/// decompressed totals (listed from the archive when possible).
/// Aborts before gigabytes move. Pure dry-run otherwise.
async fn preflight_extract(
    operator: &ArchiveOperator,
    engine: &mut StorageEngine,
    archive: &std::path::Path,
    out_dir: &std::path::Path,
    password: Option<&str>,
) -> Result<()> {
    let total = match operator.list(archive, password).await {
        Ok(meta) => meta
            .entries
            .iter()
            .filter(|e| !e.is_directory)
            .map(|e| e.size)
            .sum(),
        // Unlistable (e.g. wrong password): fall back to archive size so
        // the operation itself — not the preflight — reports the real error.
        Err(_) => std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0),
    };
    engine.check_extract_space(out_dir, total)
}

/// Preflight an in-place rewrite (modify/clean): real removed bytes from
/// the archive listing, verdict from the decision engine. A Critical
/// verdict — or an external workspace we were not given — aborts with
/// the full user-facing guidance instead of failing mid-rewrite.
async fn preflight_modify(
    operator: &ArchiveOperator,
    engine: &mut StorageEngine,
    req: &OperationRequest,
) -> Result<()> {
    let pw = req.password.as_ref().map(|p| p.expose());
    let current = std::fs::metadata(&req.archive_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let remove = match operator.list(&req.archive_path, pw).await {
        Ok(meta) => meta
            .entries
            .iter()
            .filter(|e| {
                !e.is_directory
                    && req
                        .files_to_process
                        .iter()
                        .any(|f| e.path == *f || e.path.ends_with(f.as_str()))
            })
            .map(|e| e.size)
            .sum(),
        Err(_) => 0,
    };
    let estimate = engine.calculate_estimate(current, remove, 0, None);
    match engine.decide_workspace(&req.archive_path, &estimate) {
        StorageVerdict::CriticalError { message, .. } => {
            Err(TripleWrapperError::NoWorkspace(message))
        }
        StorageVerdict::ExternalRequired {
            workspace_disk,
            message,
            ..
        } => {
            if req.workspace_override.is_some() {
                Ok(())
            } else {
                Err(TripleWrapperError::NoWorkspace(format!(
                    "{message}\nRecomendación: esta operación necesita la caché externa \
                    indicada arriba; vuelve a lanzarla con esa ruta como workspace \
                    (p. ej. {} con {:.1} GB libres)",
                    workspace_disk.mount_point.display(),
                    workspace_disk.free_gb()
                )))
            }
        }
        StorageVerdict::InternalOk { .. } => Ok(()),
    }
}

async fn cmd_disks(json: bool) -> Result<()> {
    let mut scanner = DiskScanner::new();
    let disks = scanner.scan();

    if json {
        println!("{}", serde_json::to_string_pretty(&disks)?);
    } else {
        println!(
            "{:<30} {:>10} {:>10} {:>6}  Mount Point",
            "Label", "Free", "Total", "Use%"
        );
        println!("{}", "-".repeat(80));
        for disk in disks {
            println!(
                "{:<30} {:>10} {:>10} {:>5.1}%  {}",
                truncate(&disk.label, 30),
                format_bytes(disk.free_bytes),
                format_bytes(disk.total_bytes),
                disk.usage_percent(),
                disk.mount_point.display()
            );
        }
    }
    Ok(())
}

async fn cmd_analyze(archive: String, remove: u64, add: u64, ratio: f32, json: bool) -> Result<()> {
    let mut engine = StorageEngine::new(Default::default());
    let archive_path = PathBuf::from(&archive);
    let current_size = std::fs::metadata(&archive_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let estimate = engine.calculate_estimate(current_size, remove, add, Some(ratio));
    let verdict = engine.decide_workspace(&archive_path, &estimate);

    if json {
        // Encryption detection: content flags via list, plus stderr hints
        // for header-encrypted archives (where even `7z l` needs the secret).
        // stdin is always null so 7z can never block on a password prompt.
        let mut encrypted = false;
        if let Ok(op) = ArchiveOperator::new() {
            if let Ok(meta) = op.list(&archive_path, None).await {
                encrypted = meta.encrypted;
            }
            if !encrypted {
                encrypted = op.is_encrypted(&archive_path).await;
            }
        }
        let report = analysis_report_from_verdict(&archive, &verdict, encrypted);
        let event = serde_json::json!({"kind": "analysis", "data": report});
        println!("{}", serde_json::to_string(&event)?);
    } else {
        print_verdict(&verdict);
    }
    Ok(())
}

fn analysis_report_from_verdict(
    archive: &str,
    verdict: &StorageVerdict,
    encrypted: bool,
) -> serde_json::Value {
    match verdict {
        StorageVerdict::InternalOk {
            source_disk,
            estimate,
            message,
        } => {
            serde_json::json!({
                "archive_path": archive,
                "archive_bytes": estimate.current_size,
                "used_bytes": source_disk.used_bytes,
                "needed_bytes": estimate.space_needed_for_rewrite,
                "free_bytes": source_disk.free_bytes,
                "external_free_bytes": null,
                "suggested_workspace": source_disk.mount_point,
                "status": "ok",
                "status_label": message,
                "blake3_expected": "",
                "encrypted": encrypted,
            })
        }
        StorageVerdict::ExternalRequired {
            source_disk,
            workspace_disk,
            estimate,
            workdir,
            message,
            ..
        } => {
            serde_json::json!({
                "archive_path": archive,
                "archive_bytes": estimate.current_size,
                "used_bytes": source_disk.used_bytes,
                "needed_bytes": estimate.space_needed_for_rewrite,
                "free_bytes": source_disk.free_bytes,
                "external_free_bytes": workspace_disk.free_bytes,
                "suggested_workspace": workdir,
                "status": "external",
                "status_label": message,
                "blake3_expected": "",
                "encrypted": encrypted,
            })
        }
        StorageVerdict::CriticalError {
            source_disk,
            estimate,
            best_available_gb,
            message,
        } => {
            serde_json::json!({
                "archive_path": archive,
                "archive_bytes": estimate.current_size,
                "used_bytes": source_disk.used_bytes,
                "needed_bytes": estimate.space_needed_for_rewrite,
                "free_bytes": source_disk.free_bytes,
                "external_free_bytes": (*best_available_gb * 1_073_741_824.0) as u64,
                "suggested_workspace": "",
                "status": "critical",
                "status_label": message,
                "blake3_expected": "",
                "encrypted": encrypted,
            })
        }
    }
}

fn emit_tick(
    stage: &str,
    processed: u64,
    total: u64,
    read_mbps: f64,
    write_mbps: f64,
    compress_mbps: f64,
) {
    let progress = if total > 0 {
        (processed as f64 / total as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let event = serde_json::json!({
        "kind": "tick",
        "data": {
            "stage": stage,
            "progress": progress,
            "read_mbps": read_mbps,
            "write_mbps": write_mbps,
            "compress_mbps": compress_mbps,
            "bytes_processed": processed,
            "bytes_total": total,
        }
    });
    if let Ok(line) = serde_json::to_string(&event) {
        println!("{line}");
    }
}

fn emit_error(message: &str) {
    if let Ok(line) =
        serde_json::to_string(&serde_json::json!({"kind": "error", "message": message}))
    {
        println!("{line}");
    }
}

async fn cmd_run(
    archive: String,
    workspace: String,
    operation: String,
    output: Option<String>,
    password: Option<Password>,
    resume: bool,
) -> Result<()> {
    use tokio::sync::mpsc;

    let archive_path = PathBuf::from(&archive);
    let workspace_path = PathBuf::from(&workspace);
    if !archive_path.exists() {
        emit_error(&format!("Archive not found: {archive}"));
        return Err(TripleWrapperError::ArchiveNotFound(archive_path));
    }
    if let Err(e) = std::fs::create_dir_all(&workspace_path) {
        emit_error(&format!("Cannot create workspace: {e}"));
        return Err(TripleWrapperError::Io(e));
    }

    let total = std::fs::metadata(&archive_path)
        .map(|m| m.len())
        .unwrap_or(0);
    emit_tick("Iniciando", 0, total, 0.0, 0.0, 0.0);

    let operator = ArchiveOperator::new()?;
    let pw = password_str(password.as_ref());
    match operation.to_lowercase().as_str() {
        "test" => {
            let ok = operator.test(&archive_path, pw).await?;
            if ok {
                emit_tick("Verificación completa", total, total, 0.0, 0.0, 0.0);
                Ok(())
            } else {
                emit_error("Archive integrity check failed");
                Err(TripleWrapperError::CompressionFailed(
                    "integrity check failed".into(),
                ))
            }
        }
        _ => {
            let out_dir = output.map(PathBuf::from).unwrap_or(workspace_path);
            triplewrapper_core::mount::ensure_workspace_ready(&out_dir)?;
            if let Err(e) = preflight_extract(
                &operator,
                &mut StorageEngine::new(Default::default()),
                &archive_path,
                &out_dir,
                pw,
            )
            .await
            {
                emit_error(&e.to_string());
                return Err(e);
            }
            let (tx, mut rx) = mpsc::unbounded_channel();
            let archive_clone = archive_path.clone();
            let out_clone = out_dir.clone();
            let pw_owned = password.clone();
            let handle = tokio::spawn(async move {
                let pw = pw_owned.as_ref().map(|p| p.expose());
                if resume {
                    let (stats, skipped) = operator
                        .resume_extract(&archive_clone, &out_clone, None, Some(tx), pw)
                        .await?;
                    info!("Resume skipped {} already-extracted files", skipped);
                    Ok(stats)
                } else {
                    operator
                        .extract(
                            &archive_clone,
                            &out_clone,
                            triplewrapper_core::archive::ExtractOptions {
                                progress_tx: Some(tx),
                                password: pw,
                                ..Default::default()
                            },
                        )
                        .await
                }
            });
            while let Some(t) = rx.recv().await {
                emit_tick(
                    if t.current_file.is_empty() {
                        "Extrayendo"
                    } else {
                        &t.current_file
                    },
                    t.bytes_processed,
                    total,
                    t.bytes_per_second_read,
                    t.bytes_per_second_write,
                    t.bytes_per_second_compress,
                );
            }
            let stats = handle
                .await
                .map_err(|e| TripleWrapperError::Internal(e.to_string()))??;
            emit_tick(
                "Extracción completa",
                total.max(stats.bytes_written),
                total.max(stats.bytes_written).max(1),
                stats.avg_read_mbps,
                stats.avg_write_mbps,
                stats.avg_compress_mbps,
            );
            Ok(())
        }
    }
}

async fn cmd_list(archive: String, password: Option<Password>, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let archive_path = PathBuf::from(archive);
    let metadata = operator
        .list(&archive_path, password_str(password.as_ref()))
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&metadata)?);
    } else {
        println!(
            "Archive: {} ({})",
            metadata.path.display(),
            metadata.format.default_extension()
        );
        println!("Size: {}", format_bytes(metadata.size));
        println!("Entries: {}", metadata.entries.len());
        println!();
        println!("{:<50} {:>12} {:>12} Type", "Path", "Size", "Packed");
        println!("{}", "-".repeat(90));
        for entry in &metadata.entries {
            let type_str = if entry.is_directory { "DIR" } else { "FILE" };
            println!(
                "{:<50} {:>12} {:>12} {}",
                truncate(&entry.path, 50),
                format_bytes(entry.size),
                format_bytes(entry.compressed_size),
                type_str
            );
        }
    }
    Ok(())
}

async fn cmd_extract(
    archive: String,
    output: Option<String>,
    files: Vec<String>,
    password: Option<Password>,
    resume: bool,
    json: bool,
) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let out_dir = output
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let archive_path = PathBuf::from(archive);
    // Fail fast with a clear message instead of a cryptic 7z error mid-run.
    triplewrapper_core::mount::ensure_workspace_ready(&out_dir)?;

    let pw = password_str(password.as_ref());
    // Dry-run the space math first: real decompressed totals vs output disk.
    preflight_extract(
        &operator,
        &mut StorageEngine::new(Default::default()),
        &archive_path,
        &out_dir,
        pw,
    )
    .await?;

    info!(
        "Extracting {} to {}",
        archive_path.display(),
        out_dir.display()
    );

    let stats = if resume {
        let (stats, skipped) = operator
            .resume_extract(&archive_path, &out_dir, None, None, pw)
            .await?;
        info!("Resume skipped {} already-extracted files", skipped);
        stats
    } else {
        operator
            .extract(
                &archive_path,
                &out_dir,
                triplewrapper_core::archive::ExtractOptions {
                    files: if files.is_empty() { None } else { Some(&files) },
                    password: pw,
                    ..Default::default()
                },
            )
            .await?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!(
            "Extraction completed in {}",
            format_duration(stats.duration)
        );
        println!("Bytes written: {}", format_bytes(stats.bytes_written));
        println!("Avg write speed: {:.1} MB/s", stats.avg_write_mbps);
    }
    Ok(())
}

async fn cmd_test(archive: String, password: Option<Password>, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let archive_path = PathBuf::from(archive);
    let ok = operator
        .test(&archive_path, password_str(password.as_ref()))
        .await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "ok": ok }))?
        );
    } else {
        if ok {
            println!("✓ Archive integrity OK");
        } else {
            println!("✗ Archive integrity FAILED");
            std::process::exit(1);
        }
    }
    Ok(())
}

async fn cmd_create(
    archive: String,
    files: Vec<PathBuf>,
    level: u8,
    password: Option<Password>,
    json: bool,
) -> Result<()> {
    use triplewrapper_core::compression::optimal_compression_level;

    if files.is_empty() {
        return Err(TripleWrapperError::Internal(
            "create needs at least one --files entry".into(),
        ));
    }
    let operator = ArchiveOperator::new()?;
    let archive_path = PathBuf::from(archive);
    let format = triplewrapper_core::compression::resolve_format(&archive_path).map_err(|_| {
        TripleWrapperError::InvalidFormat(
            "cannot infer format from archive suffix (try .7z/.zip/.tar.gz/...)".into(),
        )
    })?;
    let stats = operator
        .add(
            &archive_path,
            &files,
            optimal_compression_level(format, level),
            None,
            None,
            password_str(password.as_ref()),
        )
        .await?;
    // Record what we just produced in the local integrity log.
    let checksum = triplewrapper_core::integrity::IntegrityDb::open()?
        .record(&archive_path, "create")
        .await
        .map(|r| r.blake3)
        .unwrap_or_default();

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!(
            "Created {} ({}, {} files)",
            archive_path.display(),
            format_bytes(stats.bytes_written),
            files.len()
        );
        if !checksum.is_empty() {
            println!("BLAKE3: {checksum}");
        }
    }
    Ok(())
}

async fn cmd_delete(
    archive: String,
    files: Vec<String>,
    password: Option<Password>,
    json: bool,
) -> Result<()> {
    if files.is_empty() {
        return Err(TripleWrapperError::Internal(
            "delete needs at least one --files entry".into(),
        ));
    }
    let operator = ArchiveOperator::new()?;
    let archive_path = PathBuf::from(archive);
    let stats = operator
        .delete(&archive_path, &files, None, password_str(password.as_ref()))
        .await?;
    // The archive changed: refresh its integrity record.
    let checksum = triplewrapper_core::integrity::IntegrityDb::open()?
        .record(&archive_path, "delete")
        .await
        .map(|r| r.blake3)
        .unwrap_or_default();

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!(
            "Deleted {} entries from {} in {}",
            files.len(),
            archive_path.display(),
            format_duration(stats.duration)
        );
        if !checksum.is_empty() {
            println!("New BLAKE3: {checksum}");
        }
    }
    Ok(())
}

async fn cmd_devices(json: bool) -> Result<()> {
    use triplewrapper_core::mount::list_unmounted;

    let devs = list_unmounted()?;
    if json {
        println!("{}", serde_json::to_string(&devs)?);
    } else if devs.is_empty() {
        println!("No unmounted devices. Plug a drive in, or mount it in Files first.");
    } else {
        println!("{:<14} {:<10} {:>10}  Removable", "Device", "FS", "Size");
        println!("{}", "-".repeat(60));
        for d in &devs {
            println!(
                "{:<14} {:<10} {:>10}  {}",
                d.dev_path,
                d.fstype,
                format_bytes(d.size_bytes),
                if d.removable { "yes" } else { "no" },
            );
        }
    }
    Ok(())
}

async fn cmd_mount(device: String, json: bool) -> Result<()> {
    use triplewrapper_core::mount;
    use triplewrapper_core::mount::ensure_workspace_ready;

    let mount_point = mount::mount(&device)?;
    // Prove we can actually use it before reporting success.
    ensure_workspace_ready(&mount_point)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "device": device,
                "mount_point": mount_point,
            }))?
        );
    } else {
        println!("Mounted {} at {}", device, mount_point.display());
    }
    Ok(())
}

async fn cmd_watch() -> Result<()> {
    use triplewrapper_core::watch::{watch_events, DeviceEvent};

    watch_events(|event| {
        let kind = match event {
            DeviceEvent::Added => "device-added",
            DeviceEvent::Removed => "device-removed",
        };
        if let Ok(line) = serde_json::to_string(&serde_json::json!({"kind": kind})) {
            println!("{line}");
        }
    })
    .await
}

async fn cmd_unmount(device: String, json: bool) -> Result<()> {
    use triplewrapper_core::mount::unmount;

    unmount(&device)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "device": device,
                "status": "unmounted",
            }))?
        );
    } else {
        println!("Unmounted {}", device);
    }
    Ok(())
}

async fn cmd_serve() -> Result<()> {
    info!("Starting TripleWrapper core service...");
    triplewrapper_core::ipc::run_service()
        .await
        .map_err(|e| crate::TripleWrapperError::Internal(e.to_string()))
}

async fn cmd_integrity(cmd: IntegrityCommands, json: bool) -> Result<()> {
    use triplewrapper_core::integrity::IntegrityDb;

    let db = IntegrityDb::open()?;
    match cmd {
        IntegrityCommands::Record { archive, operation } => {
            let record = db.record(&PathBuf::from(&archive), &operation).await?;
            if json {
                println!("{}", serde_json::to_string(&record)?);
            } else {
                println!("Recorded BLAKE3 {} for {}", record.blake3, archive);
            }
        }
        IntegrityCommands::Check { archive } => {
            let path = PathBuf::from(&archive);
            let (matches, current, _) = db.check(&path).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "kind": "integrity-check",
                        "matches": matches,
                        "blake3": current.blake3,
                        "size_bytes": current.size_bytes,
                    }))?
                );
            } else if matches {
                println!("✓ {} matches last verified record", archive);
            } else {
                println!("✗ {} DIFFERS from last verified record", archive);
                std::process::exit(1);
            }
        }
        IntegrityCommands::Last { archive } => {
            let record = db.last_for(&PathBuf::from(&archive)).await?;
            if json {
                println!("{}", serde_json::to_string(&record)?);
            } else {
                match record {
                    Some(r) => println!(
                        "Last verified: {} ({} bytes, BLAKE3 {}…, op {})",
                        archive,
                        r.size_bytes,
                        &r.blake3[..16.min(r.blake3.len())],
                        r.operation
                    ),
                    None => println!("No integrity history for {}", archive),
                }
            }
        }
    }
    Ok(())
}

fn print_verdict(verdict: &triplewrapper_core::types::StorageVerdict) {
    use triplewrapper_core::types::StorageVerdict;

    match verdict {
        StorageVerdict::InternalOk {
            source_disk,
            estimate,
            message,
        } => {
            println!("✓ INTERNAL OK");
            println!(
                "  Source: {} ({})",
                source_disk.label,
                source_disk.mount_point.display()
            );
            println!(
                "  Free: {} / {}",
                format_bytes(source_disk.free_bytes),
                format_bytes(source_disk.total_bytes)
            );
            println!(
                "  Current: {} → Estimated: {}",
                format_bytes(estimate.current_size),
                format_bytes(estimate.estimated_final_size)
            );
            println!(
                "  Space needed: {} (with margin)",
                format_bytes(estimate.space_needed_for_rewrite)
            );
            println!("  {}", message);
        }
        StorageVerdict::ExternalRequired {
            source_disk,
            workspace_disk,
            estimate,
            workdir,
            sevenzip_workdir_param,
            message,
            requires_confirmation,
        } => {
            println!("⚠ EXTERNAL WORKSPACE REQUIRED");
            println!(
                "  Source: {} ({})",
                source_disk.label,
                source_disk.mount_point.display()
            );
            println!(
                "  Free: {} / {}",
                format_bytes(source_disk.free_bytes),
                format_bytes(source_disk.total_bytes)
            );
            println!(
                "  Workspace: {} ({})",
                workspace_disk.label,
                workspace_disk.mount_point.display()
            );
            println!(
                "  Workspace free: {}",
                format_bytes(workspace_disk.free_bytes)
            );
            println!("  Workdir: {}", workdir.display());
            println!("  7z param: {}", sevenzip_workdir_param);
            println!(
                "  Current: {} → Estimated: {}",
                format_bytes(estimate.current_size),
                format_bytes(estimate.estimated_final_size)
            );
            println!(
                "  Space needed: {}",
                format_bytes(estimate.space_needed_for_rewrite)
            );
            println!("  {}", message);
            if *requires_confirmation {
                println!("  ⚠ Requires user confirmation");
            }
        }
        StorageVerdict::CriticalError {
            source_disk,
            estimate,
            best_available_gb,
            message,
        } => {
            println!("✗ CRITICAL ERROR");
            println!(
                "  Source: {} ({})",
                source_disk.label,
                source_disk.mount_point.display()
            );
            println!("  Free: {}", format_bytes(source_disk.free_bytes));
            println!("  Best available: {:.1} GB", best_available_gb);
            println!(
                "  Space needed: {}",
                format_bytes(estimate.space_needed_for_rewrite)
            );
            println!("  {}", message);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

async fn cmd_queue(cmd: QueueCommands, json: bool) -> Result<()> {
    let config = QueueConfig {
        persistence_path: dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("triplewrapper")
            .join("queue.json"),
        ..Default::default()
    };

    let mut queue = OperationQueue::new(config)?;
    queue.init().await?;

    match cmd {
        QueueCommands::Add {
            archive,
            operation,
            output,
            priority,
            files,
            password,
        } => {
            cmd_queue_add(
                &queue,
                archive,
                operation,
                output,
                priority,
                files,
                resolve_password(password),
                json,
            )
            .await
        }
        QueueCommands::Batch { file } => cmd_queue_batch(&queue, file, json).await,
        QueueCommands::List { status, json } => cmd_queue_list(&queue, status, json).await,
        QueueCommands::Status => cmd_queue_status(&queue, json).await,
        QueueCommands::Pause => {
            queue.pause();
            println!("Queue paused");
            Ok(())
        }
        QueueCommands::Resume => {
            queue.resume();
            println!("Queue resumed");
            Ok(())
        }
        QueueCommands::PauseItem { id } => cmd_queue_pause_item(&queue, id, json).await,
        QueueCommands::ResumeItem { id } => cmd_queue_resume_item(&queue, id, json).await,
        QueueCommands::Cancel { id } => cmd_queue_cancel(&queue, id, json).await,
        QueueCommands::Retry { id } => cmd_queue_retry(&queue, id, json).await,
        QueueCommands::Cleanup { max_age_seconds } => {
            cmd_queue_cleanup(&queue, max_age_seconds, json).await
        }
        QueueCommands::Start { workers } => cmd_queue_start(&mut queue, workers, json).await,
        QueueCommands::Stop => cmd_queue_stop(&queue, json).await,
    }
}

/// CLI glue: parameters mirror `QueueCommands::Add` 1:1 by design.
#[allow(clippy::too_many_arguments)]
async fn cmd_queue_add(
    queue: &OperationQueue,
    archive: String,
    operation: String,
    output: Option<String>,
    priority_str: String,
    files: Vec<String>,
    password: Option<Password>,
    json: bool,
) -> Result<()> {
    let priority = match priority_str.to_lowercase().as_str() {
        "low" => Priority::Low,
        "normal" => Priority::Normal,
        "high" => Priority::High,
        "critical" => Priority::Critical,
        _ => Priority::Normal,
    };

    let op_type = match operation.to_lowercase().as_str() {
        "extract" => OperationType::Extract,
        "modify" => OperationType::Modify,
        "clean" => OperationType::Clean,
        "test" => OperationType::Test,
        "list" => OperationType::List,
        _ => OperationType::Extract,
    };

    if password.is_some() {
        eprintln!(
            "note: queue passwords are kept in memory only and must be re-supplied after restart"
        );
    }
    let request = OperationRequest {
        id: OperationId::new(),
        op_type,
        archive_path: PathBuf::from(archive),
        output_path: output.map(PathBuf::from),
        files_to_process: files,
        compression_level: 5,
        compression_format: None,
        workspace_override: None,
        verify_after: true,
        dry_run: false,
        password,
    };

    let id = queue.enqueue(request, Some(priority)).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": id.0,
                "status": "queued",
                "priority": priority_str,
            }))?
        );
    } else {
        println!("Operation queued with ID: {}", id.0);
    }

    Ok(())
}

async fn cmd_queue_batch(queue: &OperationQueue, file: String, _json: bool) -> Result<()> {
    let content = tokio::fs::read_to_string(file).await?;
    let requests: Vec<(OperationRequest, Option<Priority>)> = serde_json::from_str(&content)?;

    let ids = queue.enqueue_batch(requests).await?;

    println!("Batch enqueued {} operations", ids.len());
    Ok(())
}

async fn cmd_queue_list(queue: &OperationQueue, status: String, json: bool) -> Result<()> {
    let filter_status = match status.to_lowercase().as_str() {
        "pending" => Some(QueueItemStatus::Pending),
        "running" => Some(QueueItemStatus::Running),
        "paused" => Some(QueueItemStatus::Paused),
        "completed" => Some(QueueItemStatus::Completed),
        "failed" => Some(QueueItemStatus::Failed),
        "cancelled" => Some(QueueItemStatus::Cancelled),
        _ => None,
    };

    let items = if let Some(s) = filter_status {
        queue.get_by_status(s).await
    } else {
        queue.get_all().await
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        println!(
            "{:<20} {:<12} {:<10} {:<10} Archive",
            "ID", "Status", "Priority", "Operation"
        );
        println!("{}", "-".repeat(100));
        for item in &items {
            let op_str = format!("{:?}", item.request.op_type);
            let archive_name = item
                .request
                .archive_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            println!(
                "{:<20} {:<12} {:<10} {:<10} {}",
                format!("{}", item.id.0),
                format!("{:?}", item.status),
                format!("{:?}", item.priority),
                op_str,
                archive_name
            );
        }
        println!("\nTotal: {} items", items.len());
    }
    Ok(())
}

async fn cmd_queue_status(queue: &OperationQueue, json: bool) -> Result<()> {
    let pending = queue.pending_count().await;
    let running = queue.running_count().await;
    let paused = queue.is_paused();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "pending": pending,
                "running": running,
                "paused": paused,
                "max_concurrent": 2,
            }))?
        );
    } else {
        println!("Queue Status:");
        println!("  Pending:  {}", pending);
        println!("  Running:  {}", running);
        println!("  Paused:   {}", if paused { "Yes" } else { "No" });
    }
    Ok(())
}

async fn cmd_queue_pause_item(queue: &OperationQueue, id: String, json: bool) -> Result<()> {
    let op_id = OperationId(
        id.parse::<u64>()
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?,
    );
    queue.pause_item(op_id).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": id,
                "status": "paused",
            }))?
        );
    } else {
        println!("Item {} paused", id);
    }
    Ok(())
}

async fn cmd_queue_resume_item(queue: &OperationQueue, id: String, json: bool) -> Result<()> {
    let op_id = OperationId(
        id.parse::<u64>()
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?,
    );
    queue.resume_item(op_id).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": id,
                "status": "resumed",
            }))?
        );
    } else {
        println!("Item {} resumed", id);
    }
    Ok(())
}

async fn cmd_queue_cancel(queue: &OperationQueue, id: String, json: bool) -> Result<()> {
    let op_id = OperationId(
        id.parse::<u64>()
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?,
    );
    queue.cancel_item(op_id).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": id,
                "status": "cancelled",
            }))?
        );
    } else {
        println!("Item {} cancelled", id);
    }
    Ok(())
}

async fn cmd_queue_retry(queue: &OperationQueue, id: String, json: bool) -> Result<()> {
    let op_id = OperationId(
        id.parse::<u64>()
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?,
    );
    queue.retry_item(op_id).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": id,
                "status": "retrying",
            }))?
        );
    } else {
        println!("Item {} queued for retry", id);
    }
    Ok(())
}

async fn cmd_queue_cleanup(queue: &OperationQueue, max_age_seconds: u64, json: bool) -> Result<()> {
    let removed = queue
        .cleanup_old(Duration::from_secs(max_age_seconds))
        .await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "removed": removed,
            }))?
        );
    } else {
        println!("Cleaned up {} old items", removed);
    }
    Ok(())
}

async fn cmd_queue_start(queue: &mut OperationQueue, workers: usize, _json: bool) -> Result<()> {
    queue.set_max_concurrent(workers);
    println!("Starting {} queue workers...", workers);
    println!("Workers will run until 'queue stop' is called or process exits");

    // Create archive operator for processing (wrapped in Arc for sharing)
    let operator = std::sync::Arc::new(ArchiveOperator::new()?);

    // Start workers with the archive operator as processor
    queue
        .start_workers(move |item| {
            let operator = std::sync::Arc::clone(&operator);
            async move {
                // Password lives only in memory (never persisted, see serde skip).
                let pw = item.request.password.as_ref().map(|p| p.expose());
                // The external-cache dir must exist AND be writable before
                // gigabytes start flowing through 7z's `-w` flag.
                if let Some(wd) = item.request.workspace_override.as_deref() {
                    triplewrapper_core::mount::ensure_workspace_ready(wd)?;
                }
                // Dry-run the space math first: abort BEFORE gigabytes move.
                // (Space failures are terminal — retrying changes nothing.)
                {
                    let mut engine = StorageEngine::new(Default::default());
                    match item.request.op_type {
                        OperationType::Extract | OperationType::Modify => {
                            let out = item.request.output_path.clone().unwrap_or_else(|| {
                                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                            });
                            preflight_extract(
                                &operator,
                                &mut engine,
                                &item.request.archive_path,
                                &out,
                                pw,
                            )
                            .await?;
                        }
                        OperationType::Clean => {
                            preflight_modify(&operator, &mut engine, &item.request).await?;
                        }
                        OperationType::Test | OperationType::List => {}
                    }
                }
                // Process the queue item based on its operation type
                match item.request.op_type {
                    OperationType::Extract => {
                        operator
                            .extract(
                                &item.request.archive_path,
                                item.request
                                    .output_path
                                    .as_ref()
                                    .unwrap_or(&std::env::current_dir().unwrap()),
                                triplewrapper_core::archive::ExtractOptions {
                                    files: if item.request.files_to_process.is_empty() {
                                        None
                                    } else {
                                        Some(&item.request.files_to_process)
                                    },
                                    workdir: item.request.workspace_override.as_deref(),
                                    password: pw,
                                    ..Default::default()
                                },
                            )
                            .await?;
                    }
                    OperationType::Modify => {
                        // For modify, we'd need more complex logic
                        // For now, just extract as example
                        operator
                            .extract(
                                &item.request.archive_path,
                                item.request
                                    .output_path
                                    .as_ref()
                                    .unwrap_or(&std::env::current_dir().unwrap()),
                                triplewrapper_core::archive::ExtractOptions {
                                    files: if item.request.files_to_process.is_empty() {
                                        None
                                    } else {
                                        Some(&item.request.files_to_process)
                                    },
                                    workdir: item.request.workspace_override.as_deref(),
                                    password: pw,
                                    ..Default::default()
                                },
                            )
                            .await?;
                    }
                    OperationType::Test => {
                        operator.test(&item.request.archive_path, pw).await?;
                    }
                    _ => {}
                }
                Ok(())
            }
        })
        .await;

    println!(
        "Started {} queue workers (max concurrent: {})",
        workers, workers
    );
    println!("Press Ctrl+C to stop workers");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    println!("Shutdown signal received, stopping workers...");

    Ok(())
}

async fn cmd_queue_stop(queue: &OperationQueue, _json: bool) -> Result<()> {
    queue.shutdown().await;
    println!("Queue stopped gracefully");
    Ok(())
}
