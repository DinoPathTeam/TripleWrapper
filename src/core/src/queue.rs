//! Batch Operations Queue Management

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use crate::types::{OperationId, OperationRequest, ProgressTelemetry};
use crate::{Result, TripleWrapperError};

/// Priority levels for queued operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Status of a queued operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// A single item in the operation queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: OperationId,
    pub uuid: String,
    pub request: OperationRequest,
    pub priority: Priority,
    pub status: QueueItemStatus,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub retry_count: u32,
    pub max_retries: u32,
    pub progress: Option<ProgressTelemetry>,
    pub error_message: Option<String>,
}

impl QueueItem {
    pub fn new(request: OperationRequest, priority: Priority) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id: request.id,
            uuid: Uuid::new_v4().to_string(),
            request,
            priority,
            status: QueueItemStatus::Pending,
            created_at: now,
            started_at: None,
            completed_at: None,
            retry_count: 0,
            max_retries: 3,
            progress: None,
            error_message: None,
        }
    }
}

/// Queue configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    pub max_concurrent_operations: usize,
    pub default_priority: Priority,
    pub max_retries: u32,
    pub retry_delay_seconds: u64,
    pub persistence_path: PathBuf,
    pub auto_start: bool,
}

impl Default for QueueConfig {
    fn default() -> Self {
        let persistence_path = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("triplewrapper")
            .join("queue.json");

        Self {
            max_concurrent_operations: 2,
            default_priority: Priority::Normal,
            max_retries: 3,
            retry_delay_seconds: 5,
            persistence_path,
            auto_start: true,
        }
    }
}

/// Main queue manager
pub struct OperationQueue {
    config: QueueConfig,
    items: Arc<RwLock<VecDeque<QueueItem>>>,
    running: Arc<RwLock<Vec<OperationId>>>,
    paused: Arc<Mutex<bool>>,
    notify: Arc<Notify>,
    shutdown: Arc<Mutex<bool>>,
    worker_handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl OperationQueue {
    pub fn new(config: QueueConfig) -> Result<Self> {
        // Ensure persistence directory exists
        if let Some(parent) = config.persistence_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let queue = Self {
            config,
            items: Arc::new(RwLock::new(VecDeque::new())),
            running: Arc::new(RwLock::new(Vec::new())),
            paused: Arc::new(Mutex::new(false)),
            notify: Arc::new(Notify::new()),
            shutdown: Arc::new(Mutex::new(false)),
            worker_handles: Arc::new(Mutex::new(Vec::new())),
        };

        Ok(queue)
    }

    /// Initialize queue by loading persisted data
    pub async fn init(&self) -> Result<()> {
        self.load_from_disk_async().await
    }

    async fn load_from_disk_async(&self) -> Result<()> {
        if !self.config.persistence_path.exists() {
            return Ok(());
        }

        let data = tokio::fs::read(&self.config.persistence_path).await?;
        if data.is_empty() {
            return Ok(());
        }

        let items: Vec<QueueItem> = serde_json::from_slice(&data)?;

        // Only restore pending/paused items; running items become pending
        let mut queue_items = VecDeque::new();
        for mut item in items {
            match item.status {
                QueueItemStatus::Running => {
                    item.status = QueueItemStatus::Pending;
                    item.started_at = None;
                    queue_items.push_back(item);
                }
                QueueItemStatus::Paused | QueueItemStatus::Pending => {
                    queue_items.push_back(item);
                }
                _ => {} // Drop completed/failed/cancelled
            }
        }

        // Re-sort by priority
        let mut vec: Vec<_> = queue_items.into();
        vec.sort_by_key(|a| std::cmp::Reverse(a.priority));
        *self.items.write().await = vec.into();

        Ok(())
    }

    /// Add an operation to the queue
    pub async fn enqueue(
        &self,
        request: OperationRequest,
        priority: Option<Priority>,
    ) -> Result<OperationId> {
        let priority = priority.unwrap_or(self.config.default_priority);
        let mut item = QueueItem::new(request, priority);

        {
            let mut items = self.items.write().await;

            // Persistent unique id: max existing + 1 (survives process restarts,
            // unlike the in-process OperationId counter).
            let next_id = items.iter().map(|i| i.id.0).max().unwrap_or(0) + 1;
            item.id = OperationId(next_id);
            let id = item.id;

            // Insert based on priority (highest first)
            let insert_pos = items
                .iter()
                .position(|i| i.priority < priority)
                .unwrap_or(items.len());
            items.insert(insert_pos, item);

            self.notify.notify_one();
            drop(items);
            self.persist_async().await?;
            Ok(id)
        }
    }

    /// Add multiple operations at once (batch)
    pub async fn enqueue_batch(
        &self,
        requests: Vec<(OperationRequest, Option<Priority>)>,
    ) -> Result<Vec<OperationId>> {
        let mut ids = Vec::with_capacity(requests.len());

        for (request, priority) in requests {
            ids.push(self.enqueue(request, priority).await?);
        }

        self.persist_async().await?;
        Ok(ids)
    }

    /// Get next pending item (highest priority)
    pub async fn dequeue(&self) -> Option<QueueItem> {
        let mut items = self.items.write().await;

        // Find first pending item
        if let Some(pos) = items
            .iter()
            .position(|i| i.status == QueueItemStatus::Pending)
        {
            let mut item = items.remove(pos).unwrap();
            item.status = QueueItemStatus::Running;

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            item.started_at = Some(now);

            Some(item)
        } else {
            None
        }
    }

    /// Update item status
    pub async fn update_status(&self, id: OperationId, status: QueueItemStatus) -> Result<()> {
        let mut items = self.items.write().await;

        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            item.status = status;

            if status == QueueItemStatus::Completed
                || status == QueueItemStatus::Failed
                || status == QueueItemStatus::Cancelled
            {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                item.completed_at = Some(now);

                // Remove from running
                let mut running = self.running.write().await;
                running.retain(|&rid| rid != id);
            } else if status == QueueItemStatus::Running {
                let mut running = self.running.write().await;
                if !running.contains(&id) {
                    running.push(id);
                }
            }

            drop(items);
            self.persist_async().await?;
            Ok(())
        } else {
            Err(TripleWrapperError::Internal(format!(
                "Queue item not found: {}",
                id.0
            )))
        }
    }

    /// Update item progress
    pub async fn update_progress(
        &self,
        id: OperationId,
        progress: ProgressTelemetry,
    ) -> Result<()> {
        let mut items = self.items.write().await;

        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            item.progress = Some(progress);
            Ok(())
        } else {
            Err(TripleWrapperError::Internal(format!(
                "Queue item not found: {}",
                id.0
            )))
        }
    }

    /// Pause queue processing
    pub fn pause(&self) {
        *self.paused.lock().unwrap() = true;
    }

    /// Resume queue processing
    pub fn resume(&self) {
        *self.paused.lock().unwrap() = false;
        self.notify.notify_waiters();
    }

    /// Check if queue is paused
    pub fn is_paused(&self) -> bool {
        *self.paused.lock().unwrap()
    }

    /// Pause a specific item
    pub async fn pause_item(&self, id: OperationId) -> Result<()> {
        self.update_status(id, QueueItemStatus::Paused).await
    }

    /// Resume a specific item
    pub async fn resume_item(&self, id: OperationId) -> Result<()> {
        let mut items = self.items.write().await;

        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            if item.status == QueueItemStatus::Paused {
                item.status = QueueItemStatus::Pending;
                drop(items);
                self.notify.notify_one();
                self.persist_async().await?;
                Ok(())
            } else {
                Err(TripleWrapperError::Internal("Item is not paused".into()))
            }
        } else {
            Err(TripleWrapperError::Internal(format!(
                "Queue item not found: {}",
                id.0
            )))
        }
    }

    /// Cancel an item
    pub async fn cancel_item(&self, id: OperationId) -> Result<()> {
        self.update_status(id, QueueItemStatus::Cancelled).await
    }

    /// Retry a failed item
    pub async fn retry_item(&self, id: OperationId) -> Result<()> {
        let mut items = self.items.write().await;

        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            if item.status == QueueItemStatus::Failed {
                if item.retry_count < item.max_retries {
                    item.status = QueueItemStatus::Pending;
                    item.retry_count += 1;
                    item.error_message = None;
                    item.started_at = None;
                    item.completed_at = None;
                    drop(items);
                    self.notify.notify_one();
                    self.persist_async().await?;
                    Ok(())
                } else {
                    Err(TripleWrapperError::Internal("Max retries exceeded".into()))
                }
            } else {
                Err(TripleWrapperError::Internal(
                    "Item is not in failed state".into(),
                ))
            }
        } else {
            Err(TripleWrapperError::Internal(format!(
                "Queue item not found: {}",
                id.0
            )))
        }
    }

    /// Remove completed/failed/cancelled items older than duration
    pub async fn cleanup_old(&self, max_age: Duration) -> Result<usize> {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(max_age.as_secs());

        let mut items = self.items.write().await;
        let initial_len = items.len();

        items.retain(|item| {
            matches!(
                item.status,
                QueueItemStatus::Pending | QueueItemStatus::Running | QueueItemStatus::Paused
            ) || item.completed_at.unwrap_or(0) > cutoff
        });

        let removed = initial_len - items.len();
        drop(items);

        if removed > 0 {
            self.persist_async().await?;
        }

        Ok(removed)
    }

    /// Get all items
    pub async fn get_all(&self) -> Vec<QueueItem> {
        self.items.read().await.iter().cloned().collect()
    }

    /// Get items by status
    pub async fn get_by_status(&self, status: QueueItemStatus) -> Vec<QueueItem> {
        self.items
            .read()
            .await
            .iter()
            .filter(|i| i.status == status)
            .cloned()
            .collect()
    }

    /// Get currently running count
    pub async fn running_count(&self) -> usize {
        self.running.read().await.len()
    }

    /// Get pending count
    pub async fn pending_count(&self) -> usize {
        self.items
            .read()
            .await
            .iter()
            .filter(|i| i.status == QueueItemStatus::Pending)
            .count()
    }

    /// Check if queue can accept more work
    pub async fn can_accept_work(&self) -> bool {
        self.running_count().await < self.config.max_concurrent_operations
    }

    /// Set max concurrent operations (e.g. from `queue start --workers N`)
    pub fn set_max_concurrent(&mut self, n: usize) {
        self.config.max_concurrent_operations = n.max(1);
    }

    /// Persist queue to disk
    async fn persist_async(&self) -> Result<()> {
        let items = self.items.read().await;
        let data = serde_json::to_vec(&*items)?;
        tokio::fs::write(&self.config.persistence_path, data).await?;
        Ok(())
    }

    /// Start worker tasks
    pub async fn start_workers<F, Fut>(&self, processor: F)
    where
        F: Fn(QueueItem) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let processor = Arc::new(processor);

        for worker_id in 0..self.config.max_concurrent_operations {
            let queue = self.clone_for_worker();
            let processor = processor.clone();

            let handle = tokio::spawn(async move {
                queue.worker_loop(worker_id, processor).await;
            });

            self.worker_handles.lock().unwrap().push(handle);
        }
    }

    fn clone_for_worker(&self) -> WorkerQueueRef {
        WorkerQueueRef {
            config: self.config.clone(),
            items: self.items.clone(),
            running: self.running.clone(),
            paused: self.paused.clone(),
            notify: self.notify.clone(),
            shutdown: self.shutdown.clone(),
        }
    }

    /// Shutdown queue gracefully
    pub async fn shutdown(&self) {
        *self.shutdown.lock().unwrap() = true;
        self.notify.notify_waiters();

        let handles = self
            .worker_handles
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for handle in handles {
            let _ = handle.await;
        }

        // Final persist
        let _ = self.persist_async().await;
    }
}

/// Worker-safe queue reference
#[derive(Clone)]
struct WorkerQueueRef {
    config: QueueConfig,
    items: Arc<RwLock<VecDeque<QueueItem>>>,
    running: Arc<RwLock<Vec<OperationId>>>,
    paused: Arc<Mutex<bool>>,
    notify: Arc<Notify>,
    shutdown: Arc<Mutex<bool>>,
}

impl WorkerQueueRef {
    async fn worker_loop<F, Fut>(&self, _worker_id: usize, processor: Arc<F>)
    where
        F: Fn(QueueItem) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        loop {
            // Check shutdown
            if *self.shutdown.lock().unwrap() {
                break;
            }

            // Check paused
            if *self.paused.lock().unwrap() {
                self.notify.notified().await;
                continue;
            }

            // Check capacity
            let running_count = self.running.read().await.len();
            if running_count >= self.config.max_concurrent_operations {
                self.notify.notified().await;
                continue;
            }

            // Get next item
            let item = self.dequeue().await;
            let Some(mut item) = item else {
                self.notify.notified().await;
                continue;
            };

            // Mark as running
            self.running.write().await.push(item.id);

            // Process item
            let result = processor(item.clone()).await;

            // Update status based on result (auto-retry while attempts remain)
            let (status, terminal) = match result {
                Ok(_) => (QueueItemStatus::Completed, true),
                Err(e) => {
                    item.error_message = Some(e.to_string());
                    if item.retry_count < item.max_retries {
                        item.retry_count += 1;
                        (QueueItemStatus::Pending, false)
                    } else {
                        (QueueItemStatus::Failed, true)
                    }
                }
            };

            // Write back status, error and retry count
            let mut items = self.items.write().await;
            if let Some(queued_item) = items.iter_mut().find(|i| i.id == item.id) {
                queued_item.status = status;
                queued_item.error_message = item.error_message.clone();
                queued_item.retry_count = item.retry_count;
                if terminal {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    queued_item.completed_at = Some(now);
                } else {
                    queued_item.started_at = None;
                }
            }
            drop(items);

            // Remove from running
            self.running.write().await.retain(|&id| id != item.id);

            // Persist
            let _ = self.persist_async().await;

            // Notify for next item
            self.notify.notify_one();
        }
    }

    async fn dequeue(&self) -> Option<QueueItem> {
        let mut items = self.items.write().await;

        if let Some(pos) = items
            .iter()
            .position(|i| i.status == QueueItemStatus::Pending)
        {
            let mut item = items.remove(pos).unwrap();
            item.status = QueueItemStatus::Running;

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            item.started_at = Some(now);

            Some(item)
        } else {
            None
        }
    }

    async fn persist_async(&self) -> Result<()> {
        let items = self.items.read().await;
        let data = serde_json::to_vec(&*items)?;
        tokio::fs::write(&self.config.persistence_path, data).await?;
        Ok(())
    }
}

impl Clone for OperationQueue {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            items: self.items.clone(),
            running: self.running.clone(),
            paused: self.paused.clone(),
            notify: self.notify.clone(),
            shutdown: self.shutdown.clone(),
            worker_handles: self.worker_handles.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OperationRequest, OperationType};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let dir = tempdir().unwrap();
        let config = QueueConfig {
            persistence_path: dir.path().join("queue.json"),
            max_concurrent_operations: 1,
            ..Default::default()
        };

        let queue = OperationQueue::new(config).unwrap();

        let request = OperationRequest {
            id: OperationId::new(),
            op_type: OperationType::Extract,
            archive_path: PathBuf::from("/test.zip"),
            output_path: Some(PathBuf::from("/out")),
            files_to_process: vec![],
            compression_level: 5,
            compression_format: None,
            workspace_override: None,
            verify_after: true,
            dry_run: false,
            password: None,
        };

        let id = queue.enqueue(request, Some(Priority::High)).await.unwrap();
        assert_eq!(queue.pending_count().await, 1);

        // Should be able to dequeue
        let item = queue.dequeue().await.unwrap();
        assert_eq!(item.id, id);
        assert_eq!(item.priority, Priority::High);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let dir = tempdir().unwrap();
        let config = QueueConfig {
            persistence_path: dir.path().join("queue.json"),
            max_concurrent_operations: 1,
            ..Default::default()
        };

        let queue = OperationQueue::new(config).unwrap();

        // Add low then high priority
        for _ in 0..2 {
            let request = OperationRequest {
                id: OperationId::new(),
                op_type: OperationType::Extract,
                archive_path: PathBuf::from("/test.zip"),
                output_path: Some(PathBuf::from("/out")),
                files_to_process: vec![],
                compression_level: 5,
                compression_format: None,
                workspace_override: None,
                verify_after: true,
                dry_run: false,
                password: None,
            };
            queue.enqueue(request, Some(Priority::Low)).await.unwrap();
        }

        let request = OperationRequest {
            id: OperationId::new(),
            op_type: OperationType::Extract,
            archive_path: PathBuf::from("/test2.zip"),
            output_path: Some(PathBuf::from("/out")),
            files_to_process: vec![],
            compression_level: 5,
            compression_format: None,
            workspace_override: None,
            verify_after: true,
            dry_run: false,
            password: None,
        };
        queue
            .enqueue(request, Some(Priority::Critical))
            .await
            .unwrap();

        // High priority should be dequeued first
        let item = queue.dequeue().await.unwrap();
        assert_eq!(item.priority, Priority::Critical);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let dir = tempdir().unwrap();
        let config = QueueConfig {
            persistence_path: dir.path().join("queue.json"),
            ..Default::default()
        };

        let queue = OperationQueue::new(config).unwrap();

        assert!(!queue.is_paused());
        queue.pause();
        assert!(queue.is_paused());
        queue.resume();
        assert!(!queue.is_paused());
    }

    #[tokio::test]
    async fn test_batch_enqueue() {
        let dir = tempdir().unwrap();
        let config = QueueConfig {
            persistence_path: dir.path().join("queue.json"),
            ..Default::default()
        };

        let queue = OperationQueue::new(config).unwrap();

        let requests: Vec<_> = (0..5)
            .map(|_| {
                let request = OperationRequest {
                    id: OperationId::new(),
                    op_type: OperationType::Extract,
                    archive_path: PathBuf::from("/test.zip"),
                    output_path: Some(PathBuf::from("/out")),
                    files_to_process: vec![],
                    compression_level: 5,
                    compression_format: None,
                    workspace_override: None,
                    verify_after: true,
                    dry_run: false,
                    password: None,
                };
                (request, Some(Priority::Normal))
            })
            .collect();

        let ids = queue.enqueue_batch(requests).await.unwrap();
        assert_eq!(ids.len(), 5);
        assert_eq!(queue.pending_count().await, 5);
    }

    #[tokio::test]
    async fn test_pause_resume_item() {
        let dir = tempdir().unwrap();
        let config = QueueConfig {
            persistence_path: dir.path().join("queue.json"),
            ..Default::default()
        };

        let queue = OperationQueue::new(config).unwrap();

        let request = OperationRequest {
            id: OperationId::new(),
            op_type: OperationType::Extract,
            archive_path: PathBuf::from("/test.zip"),
            output_path: Some(PathBuf::from("/out")),
            files_to_process: vec![],
            compression_level: 5,
            compression_format: None,
            workspace_override: None,
            verify_after: true,
            dry_run: false,
            password: None,
        };

        let id = queue
            .enqueue(request, Some(Priority::Normal))
            .await
            .unwrap();

        // Pause item
        queue.pause_item(id).await.unwrap();
        let items = queue.get_by_status(QueueItemStatus::Paused).await;
        assert_eq!(items.len(), 1);

        // Resume item
        queue.resume_item(id).await.unwrap();
        let items = queue.get_by_status(QueueItemStatus::Pending).await;
        assert_eq!(items.len(), 1);
    }
}
