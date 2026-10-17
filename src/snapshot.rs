//! Origin-bound daily snapshots and coalesced background refresh for native commands.
//! Fetching and validation belong to the integration; failures retain published data.

use anyhow::Result;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// A persisted snapshot bound to its source, never to the active editor.
#[derive(Serialize, Deserialize)]
pub struct Snapshot<T> {
    /// Source identity, including any query scope which changes the result.
    pub origin: String,
    /// Unix seconds of the last successful fetch.
    pub fetched_at: u64,
    /// Last attempt, including failures, for callers which throttle retries.
    #[serde(default)]
    pub attempted_at: u64,
    /// Integration-owned payload.
    pub snapshot: T,
}
impl<T> Snapshot<T> {
    /// Wraps a successfully fetched and validated payload.
    pub fn new(origin: String, snapshot: T) -> Self {
        Self {
            origin,
            fetched_at: now(),
            attempted_at: now(),
            snapshot,
        }
    }
    /// Persists through the product's existing atomic private-cache writer.
    pub fn save(
        &self,
        path: Option<&Path>,
        write: impl FnOnce(&Path, &[u8]) -> Result<()>,
    ) -> Result<()>
    where
        T: Serialize,
    {
        if let Some(path) = path {
            write(path, &serde_json::to_vec(self)?)?;
        }
        Ok(())
    }
}
impl<T: DeserializeOwned> Snapshot<T> {
    /// Loads a matching source; absent, corrupt or differently bound data is missing.
    pub fn load(path: Option<&Path>, origin: &str) -> Option<Self> {
        let cache: Self = serde_json::from_slice(&std::fs::read(path?).ok()?).ok()?;
        (cache.origin == origin).then_some(cache)
    }
}
/// Current Unix seconds for snapshot timestamps.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
/// Whether a timestamp is within the daily refresh interval.
pub fn fresh(at: u64) -> bool {
    at > 0 && now().saturating_sub(at) < 24 * 60 * 60
}
/// Coalesces background refreshes, retaining an explicit force request in flight.
#[derive(Default)]
pub struct BackgroundRefresh {
    refreshing: AtomicBool,
    force_requested: AtomicBool,
}
impl BackgroundRefresh {
    /// Starts at most one worker; `refresh` must publish only to its captured binding.
    pub fn schedule(
        self: &Arc<Self>,
        name: &str,
        needed: bool,
        force: bool,
        refresh: impl Fn(bool) -> Result<()> + Send + 'static,
    ) {
        if force {
            self.force_requested.store(true, Ordering::Release);
        }
        if !(needed || force) || self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let gate = self.clone();
        let worker = move || loop {
            let force = gate.force_requested.swap(false, Ordering::AcqRel);
            if let Err(error) = refresh(force) {
                tracing::debug!(%error, "completion snapshot refresh failed; retaining cache");
            }
            gate.refreshing.store(false, Ordering::Release);
            if !gate.force_requested.load(Ordering::Acquire)
                || gate.refreshing.swap(true, Ordering::AcqRel)
            {
                break;
            }
        };
        if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(worker) {
            self.refreshing.store(false, Ordering::Release);
            tracing::debug!(%error, "could not start completion snapshot refresh");
        }
    }
}
