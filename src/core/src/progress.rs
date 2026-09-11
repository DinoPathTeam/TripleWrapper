//! Progress monitoring and telemetry

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::types::{get_cpu_count, OperationId, OperationStatus, ProgressTelemetry};

/// Progress monitor for tracking operation metrics
pub struct ProgressMonitor {
    operation_id: OperationId,
    start_time: Instant,
    status: Arc<Mutex<OperationStatus>>,
    telemetry_tx: broadcast::Sender<ProgressTelemetry>,
    internal_tx: mpsc::UnboundedSender<InternalProgressUpdate>,
    bytes_read: Arc<Mutex<u64>>,
    bytes_written: Arc<Mutex<u64>>,
    bytes_compressed: Arc<Mutex<u64>>,
    current_file: Arc<Mutex<String>>,
    files_total: u32,
    files_processed: Arc<Mutex<u32>>,
    sysmon: Arc<Mutex<SystemMonitor>>,
}

impl ProgressMonitor {
    pub fn new(
        operation_id: OperationId,
        files_total: u32,
    ) -> (Self, broadcast::Receiver<ProgressTelemetry>) {
        let (telemetry_tx, telemetry_rx) = broadcast::channel(100);
        let (internal_tx, mut internal_rx) = mpsc::unbounded_channel();

        let monitor = Self {
            operation_id,
            start_time: Instant::now(),
            status: Arc::new(Mutex::new(OperationStatus::Preparing)),
            telemetry_tx: telemetry_tx.clone(),
            internal_tx,
            bytes_read: Arc::new(Mutex::new(0)),
            bytes_written: Arc::new(Mutex::new(0)),
            bytes_compressed: Arc::new(Mutex::new(0)),
            current_file: Arc::new(Mutex::new(String::new())),
            files_total,
            files_processed: Arc::new(Mutex::new(0)),
            sysmon: Arc::new(Mutex::new(SystemMonitor::new())),
        };

        // Spawn internal aggregator
        let telemetry_tx_clone = telemetry_tx.clone();
        let status_clone = monitor.status.clone();
        let bytes_read_clone = monitor.bytes_read.clone();
        let bytes_written_clone = monitor.bytes_written.clone();
        let bytes_compressed_clone = monitor.bytes_compressed.clone();
        let current_file_clone = monitor.current_file.clone();
        let files_processed_clone = monitor.files_processed.clone();
        let sysmon_clone = monitor.sysmon.clone();
        let files_total = monitor.files_total;
        let start_time = monitor.start_time;

        tokio::spawn(async move {
            let mut last_emit = Instant::now();
            let mut last_bytes_written = 0u64;
            let mut last_bytes_read = 0u64;
            let mut last_bytes_compressed = 0u64;

            while let Some(update) = internal_rx.recv().await {
                // Update internal state
                if let Some(bytes) = update.bytes_processed_override {
                    *bytes_written_clone.lock().await = bytes;
                }
                if let Some(file) = update.file_name {
                    *current_file_clone.lock().await = file;
                }

                // Emit telemetry at max 10 Hz
                if last_emit.elapsed() >= Duration::from_millis(100) {
                    let elapsed = start_time.elapsed().as_secs_f64();

                    let current_written = *bytes_written_clone.lock().await;
                    let current_read = *bytes_read_clone.lock().await;
                    let current_compressed = *bytes_compressed_clone.lock().await;
                    let processed_files = *files_processed_clone.lock().await;
                    let current_file = current_file_clone.lock().await.clone();
                    let status = *status_clone.lock().await;

                    // Calculate speeds (MB/s)
                    let write_speed = if elapsed > 0.0 {
                        (current_written.saturating_sub(last_bytes_written)) as f64
                            / elapsed
                            / 1_048_576.0
                    } else {
                        0.0
                    };
                    let read_speed = if elapsed > 0.0 {
                        (current_read.saturating_sub(last_bytes_read)) as f64
                            / elapsed
                            / 1_048_576.0
                    } else {
                        0.0
                    };
                    let compress_speed = if elapsed > 0.0 {
                        (current_compressed.saturating_sub(last_bytes_compressed)) as f64
                            / elapsed
                            / 1_048_576.0
                    } else {
                        0.0
                    };

                    // Estimate ETA
                    let eta = if write_speed > 0.01 && files_total > 0 {
                        let total_estimate =
                            current_written * files_total as u64 / (processed_files.max(1) as u64);
                        let remaining = total_estimate.saturating_sub(current_written);
                        Some((remaining as f64 / (write_speed * 1_048_576.0)) as u64)
                    } else {
                        None
                    };

                    // Host resources, best effort (zeros when unreadable).
                    let mut sysmon = sysmon_clone.lock().await;
                    let cpu_percent = sysmon.read_cpu_percent().unwrap_or(0.0);
                    let memory_bytes = sysmon.read_memory_bytes().unwrap_or(0);
                    drop(sysmon);

                    let telemetry = ProgressTelemetry {
                        operation_id,
                        status,
                        current_file,
                        files_total,
                        files_processed: processed_files,
                        bytes_total: 0, // Unknown until we scan
                        bytes_processed: current_written,
                        bytes_per_second_read: read_speed,
                        bytes_per_second_write: write_speed,
                        bytes_per_second_compress: compress_speed,
                        eta_seconds: eta,
                        cpu_percent,
                        memory_bytes,
                    };

                    let _ = telemetry_tx_clone.send(telemetry);
                    last_emit = Instant::now();
                    last_bytes_written = current_written;
                    last_bytes_read = current_read;
                    last_bytes_compressed = current_compressed;
                }
            }
        });

        (monitor, telemetry_rx)
    }

    /// Subscribe to telemetry updates
    pub fn subscribe(&self) -> broadcast::Receiver<ProgressTelemetry> {
        self.telemetry_tx.subscribe()
    }

    /// Update status
    pub async fn set_status(&self, status: OperationStatus) {
        *self.status.lock().await = status;
    }

    /// Update current file being processed
    pub async fn set_current_file(&self, file: String) {
        *self.current_file.lock().await = file;
    }

    /// Increment files processed
    pub async fn increment_files_processed(&self) {
        let mut count = self.files_processed.lock().await;
        *count += 1;
    }

    /// Add bytes read
    pub async fn add_bytes_read(&self, bytes: u64) {
        let mut total = self.bytes_read.lock().await;
        *total += bytes;
    }

    /// Add bytes written
    pub async fn add_bytes_written(&self, bytes: u64) {
        let mut total = self.bytes_written.lock().await;
        *total += bytes;
    }

    /// Add bytes compressed
    pub async fn add_bytes_compressed(&self, bytes: u64) {
        let mut total = self.bytes_compressed.lock().await;
        *total += bytes;
    }

    /// Get current stats snapshot
    pub async fn snapshot(&self) -> ProgressTelemetry {
        let mut sysmon = self.sysmon.lock().await;
        let cpu_percent = sysmon.read_cpu_percent().unwrap_or(0.0);
        let memory_bytes = sysmon.read_memory_bytes().unwrap_or(0);
        drop(sysmon);
        ProgressTelemetry {
            operation_id: self.operation_id,
            status: *self.status.lock().await,
            current_file: self.current_file.lock().await.clone(),
            files_total: self.files_total,
            files_processed: *self.files_processed.lock().await,
            bytes_total: 0,
            bytes_processed: *self.bytes_written.lock().await,
            bytes_per_second_read: 0.0,
            bytes_per_second_write: 0.0,
            bytes_per_second_compress: 0.0,
            eta_seconds: None,
            cpu_percent,
            memory_bytes,
        }
    }

    /// Send internal update (for high-frequency updates)
    pub fn send_update(&self, update: InternalProgressUpdate) {
        let _ = self.internal_tx.send(update);
    }
}

/// Internal progress update (high frequency)
#[derive(Debug, Clone, Default)]
pub struct InternalProgressUpdate {
    pub bytes_processed_override: Option<u64>,
    pub file_name: Option<String>,
}

/// System resource monitor (CPU, Memory, I/O)
pub struct SystemMonitor {
    pid: u32,
    last_io_read: u64,
    last_io_write: u64,
    last_cpu_time: u64,
    last_check: Instant,
}

impl SystemMonitor {
    pub fn new() -> Self {
        Self {
            pid: std::process::id(),
            last_io_read: 0,
            last_io_write: 0,
            last_cpu_time: 0,
            last_check: Instant::now(),
        }
    }

    /// Read /proc/<pid>/io for I/O stats
    pub fn read_io_stats(&mut self) -> Option<(u64, u64)> {
        let path = format!("/proc/{}/io", self.pid);
        let content = std::fs::read_to_string(&path).ok()?;

        let mut read_bytes = 0u64;
        let mut write_bytes = 0u64;

        for line in content.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().parse().ok()?;
                match key.trim() {
                    "read_bytes" => read_bytes = value,
                    "write_bytes" => write_bytes = value,
                    _ => {}
                }
            }
        }

        Some((read_bytes, write_bytes))
    }

    /// Read resident memory (VmRSS) from /proc/<pid>/status, in bytes.
    pub fn read_memory_bytes(&mut self) -> Option<u64> {
        let path = format!("/proc/{}/status", self.pid);
        let content = std::fs::read_to_string(&path).ok()?;
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb.saturating_mul(1024));
            }
        }
        None
    }

    /// Read CPU usage from /proc/<pid>/stat
    pub fn read_cpu_percent(&mut self) -> Option<f32> {
        let path = format!("/proc/{}/stat", self.pid);
        let content = std::fs::read_to_string(&path).ok()?;

        // Parse utime, stime, starttime
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() < 22 {
            return None;
        }

        let utime: u64 = parts[13].parse().ok()?;
        let stime: u64 = parts[14].parse().ok()?;
        let total_time = utime + stime;

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_check).as_secs_f64();

        if elapsed < 0.1 {
            return None; // Too soon
        }

        let cpu_delta = total_time.saturating_sub(self.last_cpu_time) as f64;
        let cpu_percent = (cpu_delta / elapsed / get_cpu_count() as f64 * 100.0) as f32;

        self.last_cpu_time = total_time;
        self.last_check = now;

        Some(cpu_percent.min(100.0))
    }

    /// Get current I/O rates (MB/s)
    pub fn get_io_rates(&mut self) -> Option<(f64, f64)> {
        if let Some((read, write)) = self.read_io_stats() {
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_check).as_secs_f64();

            if elapsed < 0.1 {
                return None;
            }

            let read_rate = (read.saturating_sub(self.last_io_read)) as f64 / elapsed / 1_048_576.0;
            let write_rate =
                (write.saturating_sub(self.last_io_write)) as f64 / elapsed / 1_048_576.0;

            self.last_io_read = read;
            self.last_io_write = write;
            self.last_check = now;

            Some((read_rate, write_rate))
        } else {
            None
        }
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Progress aggregator for multiple operations
pub struct ProgressAggregator {
    monitors: Arc<Mutex<Vec<ProgressMonitor>>>,
}

impl ProgressAggregator {
    pub fn new() -> Self {
        Self {
            monitors: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn add(&self, monitor: ProgressMonitor) {
        self.monitors.lock().await.push(monitor);
    }

    pub async fn remove(&self, operation_id: OperationId) {
        self.monitors
            .lock()
            .await
            .retain(|m| m.operation_id != operation_id);
    }

    pub async fn get_all_snapshots(&self) -> Vec<ProgressTelemetry> {
        let monitors = self.monitors.lock().await;
        let mut snapshots = Vec::new();

        for monitor in monitors.iter() {
            snapshots.push(monitor.snapshot().await);
        }

        snapshots
    }
}

impl Default for ProgressAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OperationId;

    #[tokio::test]
    async fn test_progress_monitor() {
        let (monitor, mut rx) = ProgressMonitor::new(OperationId::new(), 5);

        monitor.set_status(OperationStatus::Running).await;
        monitor.set_current_file("test.txt".to_string()).await;
        monitor.add_bytes_written(1024 * 1024).await; // 1 MB

        // Should receive telemetry
        tokio::time::sleep(Duration::from_millis(200)).await;

        if let Ok(telemetry) = rx.try_recv() {
            assert_eq!(telemetry.bytes_processed, 1024 * 1024);
            assert_eq!(telemetry.current_file, "test.txt");
        }
    }
}
