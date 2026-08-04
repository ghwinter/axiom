/// Link kinds — the physical connection strategy between two ports.
///
/// The `LinkKind` is chosen by the **deployer** in the `DeploySpec`, not by the
/// machine author. The same two machines can be connected with different link
/// kinds in different deployments (e.g., `Inline` for backtest, `BoundedBuf`
/// for production).
///
/// # Algebraic view (the contract)
///
/// Despite six variants, there are only **two algebraic structures**. Every
/// `LinkKind` is a physical realisation of one of them:
///
/// | Algebra | Semantics | `LinkKind` variants |
/// |---------|-----------|---------------------|
/// | `List<T>` | bounded FIFO sequence, reader sees writes in order | `Inline`, `BoundedBuf`, `Channel`, `CasFreeRing` |
/// | `Cell<T>` | single-slot overwrite, reader sees the most recent write | `Latest`, `SharedState` |
///
/// The distinction is mathematical, not implementational: `List<T>` is a queue
/// (every enqueued element is delivered exactly once, in order), `Cell<T>` is
/// a slot (intermediate writes are silently lost, only the latest survives).
/// This is why `Latest { capacity }` ignores `capacity` — a `Cell<T>` has no
/// size, only one slot.
///
/// # Physical view (the carrier)
///
/// Each variant chooses a different physical carrier for the same algebra.
/// axiom **declares** the algebra here; a runtime adapter (e.g. `axiom-tokio`)
/// **chooses** the physical carrier. The same `LinkKind::Latest` may be:
/// - `std::sync::Mutex<Option<T>>` + `Notify` in `axiom-tokio`
/// - `AtomicPtr<T>` in a lock-free adapter
/// - `static mut Option<T>` + interrupt disable in `axiom-embedded`
/// - frame-end snapshot in `axiom-game`
///
/// axiom core does not know which; it only contracts the algebra.
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
///
/// # Runtime coverage (honest)
///
/// axiom core defines all six variants. A runtime adapter interprets the
/// variants it can physically carry — not every variant belongs in every
/// domain. A variant an adapter does not implement is **not a missing
/// feature** — it is the adapter declaring that the variant's physical
/// carrier does not belong to its domain. `CasFreeRing` does not belong in
/// tokio any more than `Channel` belongs in a `no_std` interrupt handler.

use alloc::borrow::Cow;

// ── Link kind ─────────────────────────────────────────────────────────────────

/// The physical connection strategy between two ports.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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
    ///
    /// # Algebra
    ///
    /// This is the `Cell<T>` algebra: a single slot, overwrite-on-write. It is
    /// **not** a `List<T>` with capacity 1 — a capacity-1 queue still delivers
    /// every enqueued element, while a `Cell<T>` drops intermediates.
    ///
    /// # The `capacity` field
    ///
    /// `capacity` exists for API symmetry with `BoundedBuf`/`Channel` but has
    /// **no physical effect**. A `Cell<T>` has no size — it is always one slot.
    /// Runtime adapters MUST ignore this field. The field is retained so that
    /// a `LinkKind` parsed from config (TOML/JSON) does not fail on a stray
    /// `capacity` key, and so that future variants (e.g. an N-slot "latest
    /// window") can promote it to meaningful without breaking serialisation.
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
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum ReadPolicy {
    /// Block the receiver until data is available.
    Blocking,
    /// Return immediately with an empty signal.
    NonBlocking,
}

// ── Memory region (for CasFreeRing) ───────────────────────────────────────────

/// Where a lock-free ring buffer lives in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum MemoryRegion {
    /// A fixed address known at compile time (typical in embedded systems).
    Static { addr: usize, size: usize },
    /// Heap-allocated by the runtime during deployment.
    Heap { size: usize },
}

// ── Link descriptor ───────────────────────────────────────────────────────────

/// Describes a single connection between two ports in the deployment topology.
///
/// The endpoint names use [`Cow<'static, str>`] so a `LinkSpec` can be built
/// either from compile-time `&'static str` literals (zero allocation, common in
/// code-defined topologies) or from owned [`String`]s read out of a
/// declarative config file (TOML/JSON). This is what makes `DeploySpec`
/// round-trip serializable under the `serialize` feature.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct LinkSpec {
    /// Source port, expressed as `(machine_name, port_name)`.
    pub out: (Cow<'static, str>, Cow<'static, str>),
    /// Target port, expressed as `(machine_name, port_name)`.
    pub into: (Cow<'static, str>, Cow<'static, str>),
    /// Physical connection strategy.
    pub kind: LinkKind,
}

impl LinkSpec {
    /// Create a link from two `(machine, port)` endpoints.
    ///
    /// Each component accepts anything that converts into `Cow<'static, str>`,
    /// so `&'static str` literals, `String`, and `Box<str>` all work without
    /// extra wrapping at the call site.
    pub fn new(
        out: (impl Into<Cow<'static, str>>, impl Into<Cow<'static, str>>),
        into: (impl Into<Cow<'static, str>>, impl Into<Cow<'static, str>>),
        kind: LinkKind,
    ) -> Self {
        Self {
            out: (out.0.into(), out.1.into()),
            into: (into.0.into(), into.1.into()),
            kind,
        }
    }
}
