//! IPC communication via DBus

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::{Connection, Interface, Proxy, Result as ZbusResult};
use zvariant::{OwnedObjectPath, OwnedValue, Type};
use serde::{Deserialize, Serialize};

use crate::types::*;
use crate::{Result, TripleWrapperError};

/// DBus service name
pub const SERVICE_NAME: &str = "com.triplewrapper.Core";
/// DBus object path
pub const OBJECT_PATH: &str = "/com/triplewrapper/Core";

/// Core service interface
#[derive(Clone)]
pub struct CoreService {
    engine: Arc<Mutex<crate::engine::StorageEngine>>,
    operator: Arc<Mutex<crate::archive::ArchiveOperator>>,
    active_operations: Arc<Mutex<std::collections::HashMap<OperationId, OperationHandle>>>,
}

struct OperationHandle {
    monitor: crate::progress::ProgressMonitor,
    cancel_tx: tokio::sync::oneshot::Sender<()>,
}

impl CoreService {
    pub fn new() -> Result<Self> {
        let engine = crate::engine::StorageEngine::new(Default::default());
        let operator = crate::archive::ArchiveOperator::new()?;

        Ok(Self {
            engine: Arc::new(Mutex::new(engine)),
            operator: Arc::new(Mutex::new(operator)),
            active_operations: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Get disk information
    pub async fn get_disks(&self) -> Result<Vec<DiskInfo>> {
        let mut engine = self.engine.lock().await;
        Ok(engine.refresh_disks().to_vec())
    }

    /// Get storage verdict for an operation
    pub async fn get_verdict(
        &self,
        archive_path: String,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> Result<StorageVerdict> {
        let mut engine = self.engine.lock().await;
        let path = PathBuf::from(archive_path);
        Ok(engine.quick_verdict(&path, bytes_to_remove, bytes_to_add))
    }

    /// Start an operation
    pub async fn start_operation(
        &self,
        request: OperationRequest,
    ) -> Result<OperationResponse> {
        let id = request.id;
        let archive_path = request.archive_path.clone();
        let op_type = request.op_type;
        let workdir = request.workspace_override.clone();

        // Get verdict first
        let mut engine = self.engine.lock().await;
        let estimate = engine.calculate_estimate(
            std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0),
            0, // TODO: calculate from request.files_to_process
            0,
            None,
        );
        let verdict = engine.decide_workspace(&archive_path, &estimate);
        drop(engine);

        // Check if we need confirmation for external workspace
        if matches!(verdict, StorageVerdict::ExternalRequired { .. }) && !request.dry_run {
            // In real implementation, would emit signal for GUI confirmation
            // For now, proceed if auto_select_workspace is enabled
        }

        // Create progress monitor
        let (monitor, _rx) = crate::progress::ProgressMonitor::new(id, request.files_to_process.len() as u32);
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

        // Store operation handle
        self.active_operations.lock().await.insert(id, OperationHandle { monitor, cancel_tx });

        // Spawn operation
        let operator = self.operator.clone();
        let active_ops = self.active_operations.clone();
        
        tokio::spawn(async move {
            let result = Self::run_operation(
                operator,
                request,
                verdict,
                cancel_rx,
            ).await;

            // Clean up
            active_ops.lock().await.remove(&id);
            
            // Send completion signal (via DBus)
            // TODO: emit DBus signal
        });

        Ok(OperationResponse {
            id,
            success: true,
            message: "Operation started".to_string(),
            verdict: Some(verdict),
            output_path: None,
            stats: None,
        })
    }

    async fn run_operation(
        operator: Arc<Mutex<crate::archive::ArchiveOperator>>,
        request: OperationRequest,
        verdict: StorageVerdict,
        mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<OperationStats> {
        let workdir = match &verdict {
            StorageVerdict::ExternalRequired { workdir, .. } => Some(workdir.as_path()),
            _ => None,
        };

        let op = operator.lock().await;
        let stats = match request.op_type {
            OperationType::Extract => {
                op.extract(
                    &request.archive_path,
                    request.output_path.as_ref().unwrap_or(&std::env::current_dir()?),
                    if request.files_to_process.is_empty() { None } else { Some(&request.files_to_process) },
                    workdir,
                    None, // progress_tx
                ).await?
            }
            OperationType::Modify => {
                // For modify: delete then add
                if !request.files_to_process.is_empty() {
                    op.delete(&request.archive_path, &request.files_to_process, workdir).await?;
                }
                // TODO: add new files
                OperationStats::default()
            }
            OperationType::Clean => {
                op.delete(&request.archive_path, &request.files_to_process, workdir).await?
            }
            OperationType::Test => {
                op.test(&request.archive_path).await?;
                OperationStats::default()
            }
            OperationType::List => OperationStats::default(),
        };
        
        Ok(stats)
    }

    /// Cancel an operation
    pub async fn cancel_operation(&self, id: OperationId) -> Result<bool> {
        let mut ops = self.active_operations.lock().await;
        if let Some(handle) = ops.remove(&id) {
            let _ = handle.cancel_tx.send(());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get operation progress
    pub async fn get_progress(&self, id: OperationId) -> Result<Option<ProgressTelemetry>> {
        let ops = self.active_operations.lock().await;
        if let Some(handle) = ops.get(&id) {
            Ok(Some(handle.monitor.snapshot().await))
        } else {
            Ok(None)
        }
    }

    /// Subscribe to progress updates (returns broadcast receiver)
    pub async fn subscribe_progress(&self, id: OperationId) -> Result<Option<tokio::sync::broadcast::Receiver<ProgressTelemetry>>> {
        let ops = self.active_operations.lock().await;
        if let Some(handle) = ops.get(&id) {
            Ok(Some(handle.monitor.subscribe()))
        } else {
            Ok(None)
        }
    }
}

/// DBus interface definition
#[zbus::interface(name = "com.triplewrapper.Core")]
impl CoreService {
    /// GetDisks returns list of mounted disks with space info
    async fn get_disks(&self) -> ZbusResult<Vec<DiskInfo>> {
        self.get_disks().await.map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// GetVerdict analyzes storage requirements
    async fn get_verdict(
        &self,
        archive_path: String,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> ZbusResult<StorageVerdict> {
        self.get_verdict(archive_path, bytes_to_remove, bytes_to_add)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// StartOperation begins an archive operation
    async fn start_operation(&self, request: OperationRequest) -> ZbusResult<OperationResponse> {
        self.start_operation(request).await.map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// CancelOperation stops a running operation
    async fn cancel_operation(&self, id: u64) -> ZbusResult<bool> {
        self.cancel_operation(OperationId(id)).await.map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// GetProgress returns current progress for an operation
    async fn get_progress(&self, id: u64) -> ZbusResult<Option<ProgressTelemetry>> {
        self.get_progress(OperationId(id)).await.map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Ping for health check
    async fn ping(&self) -> ZbusResult<String> {
        Ok("pong".to_string())
    }

    /// Properties
    #[zbus(property)]
    async fn version(&self) -> ZbusResult<String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }
}

/// Client for communicating with the core service
pub struct CoreClient {
    proxy: Proxy<'static>,
}

impl CoreClient {
    pub async fn new(connection: &Connection) -> ZbusResult<Self> {
        let proxy = Proxy::new(
            connection,
            SERVICE_NAME,
            OBJECT_PATH,
            "com.triplewrapper.Core",
        ).await?;

        Ok(Self { proxy })
    }

    pub async fn get_disks(&self) -> ZbusResult<Vec<DiskInfo>> {
        self.proxy.call_method("GetDisks", &()).await
    }

    pub async fn get_verdict(
        &self,
        archive_path: &str,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> ZbusResult<StorageVerdict> {
        self.proxy.call_method("GetVerdict", &(archive_path, bytes_to_remove, bytes_to_add)).await
    }

    pub async fn start_operation(&self, request: OperationRequest) -> ZbusResult<OperationResponse> {
        self.proxy.call_method("StartOperation", &(request,)).await
    }

    pub async fn cancel_operation(&self, id: OperationId) -> ZbusResult<bool> {
        self.proxy.call_method("CancelOperation", &(id.0,)).await
    }

    pub async fn get_progress(&self, id: OperationId) -> ZbusResult<Option<ProgressTelemetry>> {
        self.proxy.call_method("GetProgress", &(id.0,)).await
    }

    pub async fn ping(&self) -> ZbusResult<String> {
        self.proxy.call_method("Ping", &()).await
    }
}

/// Run the DBus service (for systemd activation or manual start)
pub async fn run_service() -> ZbusResult<()> {
    let service = CoreService::new().map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
    
    let connection = Connection::session().await?;
    
    // Register the service
    connection.request_name(SERVICE_NAME).await?;
    
    // Serve the interface
    let _ = connection.object_server().at(OBJECT_PATH, service).await?;
    
    println!("TripleWrapper core service running on {}", SERVICE_NAME);
    
    // Keep running
    tokio::signal::ctrl_c().await?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_service_creation() {
        let service = CoreService::new();
        assert!(service.is_ok());
    }
}