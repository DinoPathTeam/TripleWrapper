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
    types::{OperationId, OperationRequest, OperationType, StorageVerdict},
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
    },

    /// Extract archive
    Extract {
        #[arg(short, long)]
        archive: String,

        #[arg(short, long)]
        output: Option<String>,

        #[arg(short, long)]
        files: Vec<String>,
    },

    /// Test archive integrity
    Test {
        #[arg(short, long)]
        archive: String,
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
    },

    /// Queue management commands
    #[command(subcommand)]
    Queue(QueueCommands),
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
        Commands::List { archive } => cmd_list(archive, cli.json).await,
        Commands::Extract {
            archive,
            output,
            files,
        } => cmd_extract(archive, output, files, cli.json).await,
        Commands::Test { archive } => cmd_test(archive, cli.json).await,
        Commands::Serve => cmd_serve().await,
        Commands::Run {
            archive,
            workspace,
            operation,
            output,
        } => cmd_run(archive, workspace, operation, output).await,
        Commands::Queue(cmd) => cmd_queue(cmd, cli.json).await,
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

async fn cmd_analyze(
    archive: String,
    remove: u64,
    add: u64,
    _ratio: f32,
    json: bool,
) -> Result<()> {
    let mut engine = StorageEngine::new(Default::default());
    let archive_path = PathBuf::from(&archive);
    let verdict = engine.quick_verdict(&archive_path, remove, add);

    // Override estimate with provided ratio
    // (In real implementation, would pass ratio to quick_verdict)

    if json {
        let report = analysis_report_from_verdict(&archive, &verdict);
        let event = serde_json::json!({"kind": "analysis", "data": report});
        println!("{}", serde_json::to_string(&event)?);
    } else {
        print_verdict(&verdict);
    }
    Ok(())
}

fn analysis_report_from_verdict(archive: &str, verdict: &StorageVerdict) -> serde_json::Value {
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
    match operation.to_lowercase().as_str() {
        "test" => {
            let ok = operator.test(&archive_path).await?;
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
            let _ = std::fs::create_dir_all(&out_dir);
            let (tx, mut rx) = mpsc::unbounded_channel();
            let archive_clone = archive_path.clone();
            let out_clone = out_dir.clone();
            let handle = tokio::spawn(async move {
                operator
                    .extract(&archive_clone, &out_clone, None, None, Some(tx))
                    .await
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

async fn cmd_list(archive: String, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let archive_path = PathBuf::from(archive);
    let metadata = operator.list(&archive_path).await?;

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
    json: bool,
) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let out_dir = output
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let archive_path = PathBuf::from(archive);

    info!(
        "Extracting {} to {}",
        archive_path.display(),
        out_dir.display()
    );

    let stats = operator
        .extract(
            &archive_path,
            &out_dir,
            if files.is_empty() { None } else { Some(&files) },
            None,
            None,
        )
        .await?;

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

async fn cmd_test(archive: String, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let archive_path = PathBuf::from(archive);
    let ok = operator.test(&archive_path).await?;

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

async fn cmd_serve() -> Result<()> {
    info!("Starting TripleWrapper core service...");
    triplewrapper_core::ipc::run_service()
        .await
        .map_err(|e| crate::TripleWrapperError::Internal(e.to_string()))
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
        } => cmd_queue_add(&queue, archive, operation, output, priority, files, json).await,
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

async fn cmd_queue_add(
    queue: &OperationQueue,
    archive: String,
    operation: String,
    output: Option<String>,
    priority_str: String,
    files: Vec<String>,
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
                                if item.request.files_to_process.is_empty() {
                                    None
                                } else {
                                    Some(&item.request.files_to_process)
                                },
                                item.request.workspace_override.as_deref(),
                                None, // progress_tx - would need channel
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
                                if item.request.files_to_process.is_empty() {
                                    None
                                } else {
                                    Some(&item.request.files_to_process)
                                },
                                item.request.workspace_override.as_deref(),
                                None,
                            )
                            .await?;
                    }
                    OperationType::Test => {
                        operator.test(&item.request.archive_path).await?;
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
