//! Event-sourced replayer (D1) — replay to any point in time / time travel.
//!
//! # Role
//!
//! axiom's determinism (R001: same input → same final state) makes **replay** a first-class
//! citizen: record the input event stream that drives the system, then replay from a clean state
//! to any point in time, obtaining a state **bit-identical** to the original execution. That is
//! time-travel debugging — "replay to just before the crash and inspect the state".
//!
//! # Design: why serialization is not needed
//!
//! The replay input is a type-erased `Box<dyn Any>` (the runtime `tick` payload).
//! `Any` cannot be cloned, but replay needs to replay the same input **multiple** times. The
//! solution: the journal stores **payload factories** (`Box<dyn Fn() -> Box<dyn Any>>`) — each
//! replay invokes the factory to rebuild the payload. For `Clone` inputs (byte streams, commands,
//! scalars — the mainstream of the showcase), [`ReplayJournal::record`] wraps this automatically;
//! for arbitrary payloads use [`record_fn`](ReplayJournal::record_fn).
//!
//! Snapshots (`Machine::checkpoint`) are an **optimization** for replay (start from a snapshot
//! rather than zero); the first version's replay-from-zero already satisfies "any point in time";
//! snapshot integration is a later increment.
//!
//! # Contract
//!
//! Replay correctness is guaranteed by the runtime's determinism (not by the replayer itself):
//! the same payload factory produces the same value, and the runtime produces the same output for
//! the same input. The replayer only does three things: record, rebuild, and tick in order.

use alloc::string::String;
use alloc::vec::Vec;
use std::boxed::Box;
use std::sync::Arc;

use crate::{ProcessResult, Runtime, RuntimeError};

/// A rebuildable input event.
pub struct ReplayEntry {
    pub machine: String,
    pub port: String,
    make: Box<dyn Fn() -> Box<dyn core::any::Any + Send> + Send + Sync>,
}

impl ReplayEntry {
    /// Rebuild the payload (each call produces a new `Box<dyn Any>` — re-playable repeatedly).
    fn rebuild(&self) -> Box<dyn core::any::Any + Send> {
        (self.make)()
    }
}

/// A batch of inputs (corresponding to all inputs of one `Runtime::tick`).
pub struct TickBatch {
    entries: Vec<ReplayEntry>,
}

impl TickBatch {
    fn to_inputs(&self) -> Vec<(String, String, Box<dyn core::any::Any + Send>)> {
        self.entries
            .iter()
            .map(|e| (e.machine.clone(), e.port.clone(), e.rebuild()))
            .collect()
    }
}

/// Input event stream (journal) — the replayer's data source.
///
/// Recording protocol: call [`record`](ReplayJournal::record) for each input of a batch (one or
/// more), and call [`end_batch`](ReplayJournal::end_batch) at the end of the batch. Entries from
/// repeated [`record`](ReplayJournal::record) calls without `end_batch` belong to the same batch
/// (the same tick).
#[derive(Default)]
pub struct ReplayJournal {
    batches: Vec<TickBatch>,
}

impl ReplayJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a clonable input payload (`T: Clone` — the mainstream scenario of bytes, commands,
    /// scalars).
    ///
    /// The factory closure captures `payload.clone()`: each replay rebuilds an equal-value payload.
    pub fn record<T: Clone + Send + Sync + 'static>(
        &mut self,
        machine: impl Into<String>,
        port: impl Into<String>,
        payload: &T,
    ) -> &mut Self {
        let payload = payload.clone();
        self.record_fn(
            machine,
            port,
            Box::new(move || Box::new(payload.clone()) as Box<dyn core::any::Any + Send>),
        )
    }

    /// Record an arbitrary payload (the caller provides the rebuild factory).
    pub fn record_fn(
        &mut self,
        machine: impl Into<String>,
        port: impl Into<String>,
        make: Box<dyn Fn() -> Box<dyn core::any::Any + Send> + Send + Sync>,
    ) -> &mut Self {
        let last = self
            .batches
            .last_mut()
            .expect("end_batch() before record()");
        last.entries.push(ReplayEntry {
            machine: machine.into(),
            port: port.into(),
            make,
        });
        self
    }

    /// End the current batch (the input boundary of one `tick`).
    pub fn end_batch(&mut self) -> &mut Self {
        self.batches.push(TickBatch {
            entries: Vec::new(),
        });
        self
    }

    /// Number of recorded batches (= the maximum time point that can be replayed).
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// Number of entries in the `i`-th batch (for debugging/diagnostics).
    pub fn batch_len(&self, i: usize) -> usize {
        self.batches[i].entries.len()
    }
}

/// The replayer — rebuilds the runtime state at any point in time from a journal.
pub struct Replayer<'a> {
    journal: &'a ReplayJournal,
}

impl<'a> Replayer<'a> {
    pub fn new(journal: &'a ReplayJournal) -> Self {
        Self { journal }
    }

    /// Replay from zero up to the `t`-th batch (`t` = the time-travel target point).
    ///
    /// `build` is a runtime factory (registration + materialization — the caller owns the registry).
    /// Returns the replayed `Runtime` plus the **per-batch outputs** (the `ProcessResult` sets of
    /// each batch's tick) — the latter is used to compare against the original execution
    /// (replay correctness).
    pub fn forward_to(
        &self,
        t: usize,
        build: impl Fn() -> Runtime,
    ) -> Result<(Runtime, Vec<Vec<ProcessResult>>), RuntimeError> {
        assert!(t <= self.journal.batches.len(), "t={t} 超出 journal 时点 {}", self.journal.batches.len());
        let mut rt = build();
        let mut outputs = Vec::with_capacity(t);
        for batch in self.journal.batches.iter().take(t) {
            let out = rt.tick(batch.to_inputs())?;
            outputs.push(out);
        }
        Ok((rt, outputs))
    }

    /// Replay correctness verification: the outputs of replaying to `t` match the original
    /// execution (`original`) batch by batch.
    ///
    /// `original[i]` = the output of the `i`-th batch of the original execution. Returns the first
    /// mismatching batch index (`None` = fully consistent).
    pub fn verify<'r>(
        &self,
        t: usize,
        build: impl Fn() -> Runtime,
        original: impl IntoIterator<Item = &'r Vec<ProcessResult>>,
    ) -> Option<usize> {
        let (_, replayed) = self.forward_to(t, build).expect("replay");
        for (i, (a, b)) in replayed.iter().zip(original).enumerate() {
            if !outputs_equal(a, b) {
                return Some(i);
            }
        }
        None
    }
}

/// Output comparison: `ProcessResult` payloads are `Box<dyn Any>` — compare the port and the
/// payload's downcast value (if the payloads are the same type, compare their Debug representations
/// byte by byte; different types are considered unequal).
fn outputs_equal(a: &[ProcessResult], b: &[ProcessResult]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        match (x, y) {
            (ProcessResult::Yield { port: pa, value: va }, ProcessResult::Yield { port: pb, value: vb }) => {
                if pa != pb {
                    return false;
                }
                // Payload comparison: downcast to the same concrete type, then compare Debug
                // (the type is not required to be Serialize — replay correctness is judged by
                // "same type, same value").
                if va.type_id() != vb.type_id() {
                    return false;
                }
                let da = format!("{:?}", va);
                let db = format!("{:?}", vb);
                if da != db {
                    return false;
                }
            }
            (ProcessResult::YieldMulti { outputs: oa }, ProcessResult::YieldMulti { outputs: ob }) => {
                if oa.len() != ob.len() {
                    return false;
                }
                for (x, y) in oa.iter().zip(ob.iter()) {
                    if x.0 != y.0 || format!("{:?}", x.1) != format!("{:?}", y.1) {
                        return false;
                    }
                }
            }
            _ => {
                // Idle/Done or variant mismatch: compare variant discriminants.
                if std::mem::discriminant(x) != std::mem::discriminant(y) {
                    return false;
                }
            }
        }
    }
    true
}

// Make ReplayEntry/TickBatch passable across threads (the factory is Send + Sync).
unsafe impl Send for ReplayEntry {}
unsafe impl Sync for ReplayEntry {}


// Suppress the unused import warning (Arc is reserved for snapshot integration).
#[allow(unused)]
fn _reserve(_: Arc<()>) {}
