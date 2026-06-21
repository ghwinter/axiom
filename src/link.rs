/// Link kinds — the physical connection strategy between two ports.
///
/// The `LinkKind` is chosen by the **deployer** in the `DeploySpec`, not by the
/// machine author. The same two machines can be connected with different link
/// kinds in different deployments (e.g., `Inline` for backtest, `BoundedBuf`
/// for production).
///
/// # When to use which
///
/// | Kind | Physics | When |
/// |------|---------|------|
/// | `Inline` | Function call, zero allocation. Caller blocks. | Same-thread, Func→Func or Machine→Func. |
/// | `BoundedBuf` | Lock-based ring buffer. Three write policies, two read policies. | Cross-thread, producer-consumer, backpressure-sensitive. |
/// | `Channel` | MPSC channel (async send / blocking send). | Multiple producers, single consumer. |
/// | `Latest` | Single overwrite slot. Reader gets most recent value. | Observability, status reporting, UI refresh. |
/// | `CasFreeRing` | Lock-free SPSC ring buffer, fixed address. | Interrupt → main-loop, embedded, DMA. |
/// | `SharedState` | `Arc<RwLock<T>>`. | Config distribution, shared metrics. |

// ── Link kind ─────────────────────────────────────────────────────────────────

/// The physical connection strategy between two ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// Direct function call. Zero allocation, caller blocks.
    /// Compile-time constraint: both ends must live on the same executor thread.
    Inline,

    /// Lock-based bounded ring buffer with configurable backpressure.
    BoundedBuf {
        capacity: usize,
        write_policy: WritePolicy,
        read_policy: ReadPolicy,
    },

    /// Multi-producer, single-consumer channel.
    Channel {
        capacity: usize,
        /// If `true`, senders drop the message when the channel is full
        /// (fire-and-forget). If `false`, senders block (backpressure).
        drop_when_full: bool,
    },

    /// Single overwrite slot. Reader sees the most recently written value.
    /// Suitable for "current status" feeds.
    Latest {
        capacity: usize,
    },

    /// Lock-free single-producer single-consumer ring buffer.
    /// The storage region is fixed at deploy time (static address or pre-allocated).
    CasFreeRing {
        capacity: usize,
        storage: MemoryRegion,
    },

    /// Shared state guarded by a read-write lock.
    SharedState,
}

// ── Write/Read policies (for BoundedBuf) ──────────────────────────────────────

/// Behaviour when the buffer is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePolicy {
    /// Block the sender until a slot becomes available.
    /// Provides natural backpressure.
    Blocking,
    /// Drop the new item and return an error.
    Dropping,
    /// Overwrite the oldest item (ring-buffer semantics).
    Overwriting,
}

/// Behaviour when the buffer is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPolicy {
    /// Block the receiver until data is available.
    Blocking,
    /// Return immediately with an empty signal.
    NonBlocking,
}

// ── Memory region (for CasFreeRing) ───────────────────────────────────────────

/// Where a lock-free ring buffer lives in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryRegion {
    /// A fixed address known at compile time (typical in embedded systems).
    Static { addr: usize, size: usize },
    /// Heap-allocated by the runtime during deployment.
    Heap { size: usize },
}

// ── Link descriptor ───────────────────────────────────────────────────────────

/// Describes a single connection between two ports in the deployment topology.
#[derive(Debug, Clone)]
pub struct LinkSpec {
    /// Source port, expressed as `(machine_name, port_name)`.
    pub out: (&'static str, &'static str),
    /// Target port, expressed as `(machine_name, port_name)`.
    pub into: (&'static str, &'static str),
    /// Physical connection strategy.
    pub kind: LinkKind,
}

impl LinkSpec {
    pub fn new(
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
        kind: LinkKind,
    ) -> Self {
        Self { out, into, kind }
    }
}
