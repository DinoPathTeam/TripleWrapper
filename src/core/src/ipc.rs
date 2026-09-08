//! IPC communication via DBus

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::{Connection, fdo};
use serde::{Deserialize, Serialize};
use zvariant_derive::Type;

use crate::types::*;
use crate::{Result, TripleWrapperError};

/// DBus service name
pub const SERVICE_NAME: &str = "com.triplewrapper.Core";
/// DBus object path
pub const OBJECT_PATH: &str = "/com/triplewrapper/Core";

/// DBus-friendly storage verdict (simplified for DBus transport)
#[derive(Debug, Clone, Serialize, Deserialize, zvariant_derive::Type)]
pub struct DBusStorageVerdict {
    pub verdict: String,  // "internal_ok", "external_required", "critical_error"
    pub source_disk: DiskInfo,
    pub workspace_disk: DiskInfo,  // empty if not applicable
    pub has_workspace_disk: bool,
    pub estimate: CompressionEstimate,
    pub has_estimate: bool,
    pub workdir: String,  // empty if not applicable
    pub sevenzip_workdir_param: String,  // empty if not applicable
    pub message: String,
    pub requires_confirmation: bool,
}

impl From<StorageVerdict> for DBusStorageVerdict {
    fn from(v: StorageVerdict) -> Self {
        match v {
            StorageVerdict::InternalOk { source_disk, estimate, message } => Self {
                verdict: "internal_ok".to_string(),
                source_disk,
                workspace_disk: DiskInfo::default(),
                has_workspace_disk: false,
                estimate,
                has_estimate: true,
                workdir: String::new(),
                sevenzip_workdir_param: String::new(),
                message,
                requires_confirmation: false,
            },
            StorageVerdict::ExternalRequired { source_disk, workspace_disk, estimate, workdir, sevenzip_workdir_param, message, requires_confirmation } => Self {
                verdict: "external_required".to_string(),
                source_disk,
                workspace_disk,
                has_workspace_disk: true,
                estimate,
                has_estimate: true,
                workdir: workdir.to_string_lossy().to_string(),
                sevenzip_workdir_param,
                message,
                requires_confirmation,
            },
            StorageVerdict::CriticalError { source_disk, estimate, best_available_gb, message } => Self {
                verdict: "critical_error".to_string(),
                source_disk,
                workspace_disk: DiskInfo::default(),
                has_workspace_disk: false,
                estimate,
                has_estimate: true,
                workdir: String::new(),
                sevenzip_workdir_param: String::new(),
                message: format!("{} (best available: {:.1} GB)", message, best_available_gb),
                requires_confirmation: false,
            },
        }
    }
}

/// Internal service logic (not exposed via DBus)
pub struct CoreServiceInternal {
    engine: Arc<Mutex<crate::engine::StorageEngine>>,
    operator: Arc<Mutex<crate::archive::ArchiveOperator>>,
}

impl CoreServiceInternal {
    pub fn new() -> Result<Self> {
        let engine = crate::engine::StorageEngine::new(Default::default());
        let operator = crate::archive::ArchiveOperator::new()?;

        Ok(Self {
            engine: Arc::new(Mutex::new(engine)),
            operator: Arc::new(Mutex::new(operator)),
        })
    }

    async fn get_disks(&self) -> Result<Vec<DiskInfo>> {
        let mut engine = self.engine.lock().await;
        Ok(engine.refresh_disks().to_vec())
    }

    async fn get_verdict(
        &self,
        archive_path: String,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> Result<DBusStorageVerdict> {
        let mut engine = self.engine.lock().await;
        let path = PathBuf::from(archive_path);
        let verdict = engine.quick_verdict(&path, bytes_to_remove, bytes_to_add);
        Ok(verdict.into())
    }
}

/// DBus service wrapper
#[derive(Clone)]
pub struct CoreService {
    internal: Arc<CoreServiceInternal>,
}

impl CoreService {
    pub fn new() -> Result<Self> {
        let internal = CoreServiceInternal::new()?;
        Ok(Self {
            internal: Arc::new(internal),
        })
    }
}

/// DBus interface definition
#[zbus::interface(name = "com.triplewrapper.Core")]
impl CoreService {
    /// GetDisks returns list of mounted disks with space info
    async fn get_disks(&self) -> fdo::Result<Vec<DiskInfo>> {
        self.internal.get_disks().await
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// GetVerdict analyzes storage requirements
    async fn get_verdict(
        &self,
        archive_path: String,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> fdo::Result<DBusStorageVerdict> {
        self.internal.get_verdict(archive_path, bytes_to_remove, bytes_to_add).await
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    /// Ping for health check
    async fn ping(&self) -> fdo::Result<String> {
        Ok("pong".to_string())
    }

    /// Properties
    #[zbus(property)]
    async fn version(&self) -> fdo::Result<String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }
}

/// Client for communicating with the core service
pub struct CoreClient {
    connection: Connection,
}

impl CoreClient {
    pub async fn new() -> zbus::Result<Self> {
        let connection = Connection::session().await?;
        Ok(Self { connection })
    }

    pub async fn get_disks(&self) -> zbus::Result<Vec<DiskInfo>> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            SERVICE_NAME,
            OBJECT_PATH,
            "com.triplewrapper.Core",
        ).await?;
        
        let reply = proxy.call_method("GetDisks", &()).await?;
        reply.body().deserialize()
    }

    pub async fn get_verdict(
        &self,
        archive_path: &str,
        bytes_to_remove: u64,
        bytes_to_add: u64,
    ) -> zbus::Result<DBusStorageVerdict> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            SERVICE_NAME,
            OBJECT_PATH,
            "com.triplewrapper.Core",
        ).await?;
        
        let reply = proxy.call_method("GetVerdict", &(archive_path, bytes_to_remove, bytes_to_add)).await?;
        reply.body().deserialize()
    }

    pub async fn ping(&self) -> zbus::Result<String> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            SERVICE_NAME,
            OBJECT_PATH,
            "com.triplewrapper.Core",
        ).await?;
        
        let reply = proxy.call_method("Ping", &()).await?;
        reply.body().deserialize()
    }
}

/// Run the DBus service (for systemd activation or manual start)
pub async fn run_service() -> zbus::Result<()> {
    let service = CoreService::new().map_err(|e| fdo::Error::Failed(e.to_string()))?;
    
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