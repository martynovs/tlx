//! Search history, indexed by stable shard IDs rather than worker count.
//!
//! A worker leases one appendable shard. At a checkpoint barrier it returns
//! that shard, so every reader sees the same complete directory, including
//! shards left idle after recovery with fewer workers. Leases are restored
//! only after the checkpoint commits. Trace replay and graph construction
//! consume this same directory when the search ends.

use std::sync::Arc;

use parking_lot::Mutex;

use super::edges::{AtomBits, EdgeRec};
use super::tracelog::TraceLog;

pub(super) struct Shard<'m, S> {
    pub(super) id: usize,
    pub(super) log: TraceLog,
    pub(super) edges: Option<EdgeRec<'m, S>>,
}

impl<S> Default for Shard<'_, S> {
    /// An empty placeholder while the real shard is in the directory.
    fn default() -> Self {
        Self { id: usize::MAX, log: TraceLog::default(), edges: None }
    }
}

pub(super) struct History<'m, S> {
    shards: Vec<Arc<Mutex<Option<Shard<'m, S>>>>>,
}

impl<'m, S> History<'m, S> {
    pub(super) fn new() -> Self {
        Self { shards: Vec::new() }
    }

    pub(super) fn len(&self) -> usize {
        self.shards.len()
    }

    pub(super) fn push(&mut self, log: TraceLog, edges: Option<EdgeRec<'m, S>>) {
        let id = self.len();
        self.shards.push(Arc::new(Mutex::new(Some(Shard { id, log, edges }))));
    }

    /// Initial/resumed logs and new workers get fresh edge recorders;
    /// recovered shards already contain both logs and edge recorders.
    pub(super) fn prepare(&mut self, logs: Vec<TraceLog>, workers: usize, atoms: Option<&'m (dyn AtomBits<S> + Sync)>, bytes: usize) {
        for log in logs {
            self.push(log, atoms.map(|a| EdgeRec::new(a, bytes)));
        }
        while self.len() < workers {
            self.push(TraceLog::new(bytes), atoms.map(|a| EdgeRec::new(a, bytes)));
        }
    }

    pub(super) fn take(&self, id: usize) -> Shard<'m, S> {
        self.shards[id].lock().take().expect("a published history shard")
    }

    pub(super) fn put(&self, shard: Shard<'m, S>) {
        let mut slot = self.shards[shard.id].lock();
        assert!(slot.is_none(), "a shard can only have one owner");
        *slot = Some(shard);
    }

    /// Read handles for a complete snapshot. The caller holds the level
    /// barrier: no worker can lease or append a shard until writing ends.
    pub(super) fn snapshot(&self) -> Vec<Arc<Mutex<Option<Shard<'m, S>>>>> {
        debug_assert!(self.shards.iter().all(|s| s.lock().is_some()));
        self.shards.clone()
    }

    pub(super) fn into_parts(self) -> (Vec<TraceLog>, Vec<EdgeRec<'m, S>>, bool) {
        let mut logs = Vec::with_capacity(self.len());
        let mut edges = Vec::new();
        for id in 0..self.len() {
            let shard = self.take(id);
            logs.push(shard.log);
            edges.extend(shard.edges);
        }
        let complete = edges.len() == logs.len();
        (logs, edges, complete)
    }
}
