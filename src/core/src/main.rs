//! TripleWrapper Core - Main binary

use clap::{Parser, Subcommand};
use tracing::{info, error, Level};
use tracing_subscriber::{fmt, EnvFilter};

use triplewrapper_core::{
    engine::StorageEngine,
    archive::ArchiveOperator,
    disk::DiskScanner,
    types::{OperationType, OperationRequest, CompressionEstimate, StorageVerdict},
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
        .init();

    match cli.command {
        Commands::Disks => cmd_disks(cli.json).await,
        Commands::Analyze { archive, remove, add, ratio } => {
            cmd_analyze(archive, remove, add, ratio, cli.json).await
        }
        Commands::List { archive } => cmd_list(archive, cli.json).await,
        Commands::Extract { archive, output, files } => {
            cmd_extract(archive, output, files, cli.json).await
        }
        Commands::Test { archive } => cmd_test(archive, cli.json).await,
        Commands::Serve => cmd_serve().await,
    }
}

async fn cmd_disks(json: bool) -> Result<()> {
    let mut scanner = DiskScanner::new();
    let disks = scanner.scan();

    if json {
        println!("{}", serde_json::to_string_pretty(&disks)?);
    } else {
        println!("{:<30} {:>10} {:>10} {:>6}  {}", "Label", "Free", "Total", "Use%", "Mount Point");
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
    let verdict = engine.quick_verdict(&archive.into(), remove, add);
    
    // Override estimate with provided ratio
    // (In real implementation, would pass ratio to quick_verdict)

    if json {
        println!("{}", serde_json::to_string_pretty(&verdict)?);
    } else {
        print_verdict(&verdict);
    }
    Ok(())
}

async fn cmd_list(archive: String, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let metadata = operator.list(&archive.into()).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&metadata)?);
    } else {
        println!("Archive: {} ({})", metadata.path.display(), metadata.format.default_extension());
        println!("Size: {}", format_bytes(metadata.size));
        println!("Entries: {}", metadata.entries.len());
        println!();
        println!("{:<50} {:>12} {:>12} {}", "Path", "Size", "Packed", "Type");
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

async fn cmd_extract(archive: String, output: Option<String>, files: Vec<String>, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let out_dir = output.map(std::path::PathBuf::from).unwrap_or_else(std::env::current_dir);
    
    info!("Extracting {} to {}", archive, out_dir.display());
    
    let stats = operator.extract(
        &archive.into(),
        &out_dir,
        if files.is_empty() { None } else { Some(&files) },
        None,
        None,
    ).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!("Extraction completed in {}", format_duration(stats.duration));
        println!("Bytes written: {}", format_bytes(stats.bytes_written));
        println!("Avg write speed: {:.1} MB/s", stats.avg_write_mbps);
    }
    Ok(())
}

async fn cmd_test(archive: String, json: bool) -> Result<()> {
    let operator = ArchiveOperator::new()?;
    let ok = operator.test(&archive.into()).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "ok": ok }))?);
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
    triplewrapper_core::ipc::run_service().await.map_err(|e| crate::TripleWrapperError::Internal(e.to_string()))
}

fn print_verdict(verdict: &triplewrapper_core::types::StorageVerdict) {
    use triplewrapper_core::types::StorageVerdict;
    
    match verdict {
        StorageVerdict::InternalOk { source_disk, estimate, message } => {
            println!("✓ INTERNAL OK");
            println!("  Source: {} ({})", source_disk.label, source_disk.mount_point.display());
            println!("  Free: {} / {}", format_bytes(source_disk.free_bytes), format_bytes(source_disk.total_bytes));
            println!("  Current: {} → Estimated: {}", format_bytes(estimate.current_size), format_bytes(estimate.estimated_final_size));
            println!("  Space needed: {} (with margin)", format_bytes(estimate.space_needed_for_rewrite));
            println!("  {}", message);
        }
        StorageVerdict::ExternalRequired { source_disk, workspace_disk, estimate, workdir, sevenzip_workdir_param, message, requires_confirmation } => {
            println!("⚠ EXTERNAL WORKSPACE REQUIRED");
            println!("  Source: {} ({})", source_disk.label, source_disk.mount_point.display());
            println!("  Free: {} / {}", format_bytes(source_disk.free_bytes), format_bytes(source_disk.total_bytes));
            println!("  Workspace: {} ({})", workspace_disk.label, workspace_disk.mount_point.display());
            println!("  Workspace free: {}", format_bytes(workspace_disk.free_bytes));
            println!("  Workdir: {}", workdir.display());
            println!("  7z param: {}", sevenzip_workdir_param);
            println!("  Current: {} → Estimated: {}", format_bytes(estimate.current_size), format_bytes(estimate.estimated_final_size));
            println!("  Space needed: {}", format_bytes(estimate.space_needed_for_rewrite));
            println!("  {}", message);
            if *requires_confirmation {
                println!("  ⚠ Requires user confirmation");
            }
        }
        StorageVerdict::CriticalError { source_disk, estimate, best_available_gb, message } => {
            println!("✗ CRITICAL ERROR");
            println!("  Source: {} ({})", source_disk.label, source_disk.mount_point.display());
            println!("  Free: {}", format_bytes(source_disk.free_bytes));
            println!("  Best available: {:.1} GB", best_available_gb);
            println!("  Space needed: {}", format_bytes(estimate.space_needed_for_rewrite));
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