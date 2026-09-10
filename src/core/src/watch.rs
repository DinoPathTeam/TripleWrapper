//! Live USB hotplug monitoring via UDisks2 D-Bus signals.
//!
//! Subscribes to `InterfacesAdded` / `InterfacesRemoved` on the UDisks2
//! object manager (system bus). When a block/filesystem interface appears
//! or vanishes — i.e. you plug or unplug a drive — the caller learns about
//! it immediately instead of waiting for the next poll.
//!
//! The event carries no device details on purpose: receivers re-scan with
//! [`crate::mount::list_unmounted`], which is the single source of truth.
//! Authentication is irrelevant here (read-only signal subscription).

use std::collections::HashMap;

use futures_util::StreamExt;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream, MessageType};

use crate::{Result, TripleWrapperError};

/// UDisks2 object-manager path.
const UDISKS2_PATH: &str = "/org/freedesktop/UDisks2";
/// UDisks2 object-manager interface emitting the signals we want.
const OBJECT_MANAGER_IFACE: &str = "org.freedesktop.DBus.ObjectManager";

/// Interfaces proving the object is storage (not a job, drive metadata…).
const STORAGE_IFACES: &[&str] = &[
    "org.freedesktop.UDisks2.Filesystem",
    "org.freedesktop.UDisks2.Block",
    "org.freedesktop.UDisks2.Partition",
    "org.freedesktop.UDisks2.Encrypted",
];

/// A plug/unplug event worth rescanning for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Added,
    Removed,
}

/// Pure classifier: does this ObjectManager signal concern storage?
/// Unit-tested without a bus.
fn classify_signal(member: &str, interfaces: &[String]) -> Option<DeviceEvent> {
    let storage = interfaces
        .iter()
        .any(|i| STORAGE_IFACES.contains(&i.as_str()));
    if !storage {
        return None;
    }
    match member {
        "InterfacesAdded" => Some(DeviceEvent::Added),
        "InterfacesRemoved" => Some(DeviceEvent::Removed),
        _ => None,
    }
}

/// Handle one incoming D-Bus message. Returns an event when the message
/// is a storage-related InterfacesAdded/Removed signal.
async fn handle_message(msg: zbus::Message) -> Option<DeviceEvent> {
    let header = msg.header();
    let member = header.member()?.as_str().to_string();
    if header.message_type() != MessageType::Signal {
        return None;
    }
    if member != "InterfacesAdded" && member != "InterfacesRemoved" {
        return None;
    }
    // Body: (object_path, a{sa{sv}}). We only need interface names.
    let (_path, ifaces): (
        zbus::zvariant::OwnedObjectPath,
        HashMap<String, HashMap<String, OwnedValue>>,
    ) = msg.body().deserialize().ok()?;
    let names: Vec<String> = ifaces.keys().cloned().collect();
    classify_signal(&member, &names)
}

/// Stream device events until cancelled. Each event means "re-scan now".
pub async fn watch_events<F>(mut on_event: F) -> Result<()>
where
    F: FnMut(DeviceEvent) + Send,
{
    let conn = Connection::system().await.map_err(|e| {
        TripleWrapperError::Internal(format!("no system bus (UDisks2 unavailable): {e}"))
    })?;
    let mut streams = Vec::new();
    for member in ["InterfacesAdded", "InterfacesRemoved"] {
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(OBJECT_MANAGER_IFACE)
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?
            .member(member)
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?
            .path(UDISKS2_PATH)
            .map_err(|e| TripleWrapperError::Internal(e.to_string()))?
            .build();
        let stream = MessageStream::for_match_rule(rule, &conn, Some(32))
            .await
            .map_err(|e| {
                TripleWrapperError::Internal(format!("cannot subscribe to UDisks2: {e}"))
            })?;
        streams.push(stream);
    }
    let [mut added, mut removed] = streams
        .try_into()
        .map_err(|_| TripleWrapperError::Internal("UDisks2 subscription failed".into()))?;
    loop {
        let msg = tokio::select! {
            msg = added.next() => msg,
            msg = removed.next() => msg,
        };
        let msg = match msg {
            Some(Ok(m)) => m,
            _ => continue,
        };
        if let Some(event) = handle_message(msg).await {
            on_event(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_added_is_event() {
        assert_eq!(
            classify_signal(
                "InterfacesAdded",
                &[
                    "org.freedesktop.UDisks2.Block".into(),
                    "org.freedesktop.UDisks2.Filesystem".into()
                ]
            ),
            Some(DeviceEvent::Added)
        );
    }

    #[test]
    fn test_partition_removed_is_event() {
        assert_eq!(
            classify_signal(
                "InterfacesRemoved",
                &["org.freedesktop.UDisks2.Partition".into()]
            ),
            Some(DeviceEvent::Removed)
        );
    }

    #[test]
    fn test_job_signals_are_ignored() {
        assert_eq!(
            classify_signal("InterfacesAdded", &["org.freedesktop.UDisks2.Job".into()]),
            None
        );
    }

    #[test]
    fn test_unknown_member_ignored() {
        assert_eq!(
            classify_signal(
                "PropertiesChanged",
                &["org.freedesktop.UDisks2.Filesystem".into()]
            ),
            None
        );
    }
}
