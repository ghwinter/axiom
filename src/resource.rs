/// **Maturity: stable** (the stable core, main subject of the current refactor).
///
/// Resource classes for lifecycle-aware resource tracking.
///
/// Every `Machine` consumes resources. Some are reclaimable when the machine
/// stops; others are permanent. This module codifies the distinction.

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use alloc::borrow::Cow;

// ── Resource class ────────────────────────────────────────────────────────────

/// Classification of a resource by its reclaimability.
///
/// Note: variants carry `&'static str`, so this type is not directly
/// `Deserialize`-able without an owning interner. `Serialize` is trivial.
/// Full owned variants are a future addition (see docs §14.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceClass {
    /// Code segment, type metadata, factory registration.
    /// Persists for the process lifetime. Not reclaimable.
    /// Only instance data (heap) is freed on machine stop.
    Static,

    /// Heap-allocated state, buffers, channels, Arcs.
    /// Reclaimed by `Drop` when the machine's `State` is dropped.
    DynamicHeap {
        /// Estimated size in bytes (for pool sizing decisions).
        estimated_bytes: usize,
    },

    /// OS-level resources: file descriptors, sockets, memory-mapped regions.
    /// Reclaimed by explicit `close()` / `munmap()` calls.
    OsResource {
        /// Human-readable description (e.g., "tcp_socket", "mmap_file").
        kind: &'static str,
    },

    /// Dedicated OS thread.
    /// Reclaimed by `thread.join()`.
    Thread {
        /// Thread name for debugging.
        name: &'static str,
    },

    /// Subprocess.
    /// Reclaimed by `SIGTERM` + `wait()`.
    Subprocess {
        /// Executable path.
        executable: String,
    },
}

// ── Physical spec (deploy-time) ───────────────────────────────────────────────

/// CPU core affinity request — deploy-time declaration, honored by the runtime.
///
/// Hard real-time deployments (architecture.md §"Hard real-time") pin a
/// `CpuBound` machine to specific cores. The runtime declares whether it can
/// honor pinning ([`crate::runtime_contract::PhysicalBudget::cpu_affinity`] /
/// `cpu_exclusive`); a blueprint that requests affinity the runtime cannot
/// provide is rejected by
/// [`crate::runtime_contract::RuntimeContract::check_spec`] before deployment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum CpuAffinity {
    /// No affinity request — the OS scheduler decides (default).
    #[default]
    None,
    /// Run on any of the listed cores, shared with other threads.
    Allowed(Vec<u32>),
    /// Run exclusively on the listed cores — no other thread may be
    /// scheduled there for the machine's lifetime.
    Exclusive(Vec<u32>),
}

/// Huge-page requirement for the machine's working memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum HugePages {
    /// No huge-page requirement (default).
    #[default]
    None,
    /// 2 MiB pages (x86-64 `MAP_HUGETLB` 2 MiB, ARM64 2 MiB).
    Size2MiB,
    /// 1 GiB pages (x86-64 1 GiB hugepages).
    Size1GiB,
}

/// Required SIMD instruction-set features for the machine's hot path.
///
/// Feature names follow LLVM / Rust `target_feature` naming (e.g. `"avx2"`,
/// `"sse4.2"`, `"neon"`). The runtime declares the features its build target
/// provides ([`crate::runtime_contract::PhysicalBudget::simd_features`]); a
/// machine requiring a feature the runtime cannot provide is rejected by
/// `check_spec`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SimdRequirement {
    /// Required instruction sets (e.g. `["avx2", "fma"]`).
    pub features: Vec<Cow<'static, str>>,
}

/// Physical resource requirements for a `Machine` instance.
///
/// This is specified by the **deployer** in the `DynamicTopology`, not by the
/// machine author. The same machine type can have different physical specs
/// in different deployments (backtest vs. production).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct MachinePhysicalSpec {
    /// Execution strategy (async, dedicated thread, thread pool, subprocess).
    pub execution: ExecutionHint,

    /// Expected heap usage of `State` (for pool sizing).
    pub state_heap_bytes: usize,

    /// Whether `State` should be cache-line aligned.
    pub cache_line_align: bool,

    /// Whether the machine is deterministic (safe for replay).
    pub deterministic: bool,

    /// Maximum acceptable `cleanup()` latency in microseconds.
    pub max_cleanup_latency_us: u64,

    /// Expected per-message processing latency in microseconds.
    ///
    /// Used by critical-path / latency-budget analysis
    /// ([`critical_path_latency`](crate::analysis::critical_path_latency)).
    /// `0` means "undeclared" — analysis treats it as zero latency.
    /// `#[serde(default)]`: when omitted in a config file, treated as undeclared (`0`).
    #[cfg_attr(feature = "serialize", serde(default))]
    pub per_message_latency_us: u64,

    /// CPU core affinity request (hard real-time deployments).
    ///
    /// `#[serde(default)]`: omitted in a config file → [`CpuAffinity::None`].
    #[cfg_attr(feature = "serialize", serde(default))]
    pub cpu_affinity: CpuAffinity,

    /// Preferred NUMA node (0-based) for the machine's memory.
    /// `None` = no preference. Honored only when the runtime declares
    /// [`crate::runtime_contract::PhysicalBudget::numa`].
    #[cfg_attr(feature = "serialize", serde(default))]
    pub numa_node: Option<u32>,

    /// Huge-page requirement for `State` / working memory.
    #[cfg_attr(feature = "serialize", serde(default))]
    pub huge_pages: HugePages,

    /// Required SIMD instruction sets for the hot path.
    #[cfg_attr(feature = "serialize", serde(default))]
    pub simd: Option<SimdRequirement>,
}

impl Default for MachinePhysicalSpec {
    fn default() -> Self {
        Self {
            execution: ExecutionHint::Async,
            state_heap_bytes: 4096,
            cache_line_align: false,
            deterministic: false,
            max_cleanup_latency_us: 10_000,
            per_message_latency_us: 0,
            cpu_affinity: CpuAffinity::None,
            numa_node: None,
            huge_pages: HugePages::None,
            simd: None,
        }
    }
}

// ── Execution hints ───────────────────────────────────────────────────────────

/// Execution strategy for a `Machine` instance.
///
/// Chosen by the deployer. The same machine type can be deployed with
/// different execution hints in different contexts.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum ExecutionHint {
    /// Async, cooperative multitasking (Tokio, Embassy).
    Async,

    /// Dedicated OS thread.
    CpuBound,

    /// N dedicated OS threads.
    CpuBoundN(usize),

    /// Private bounded thread pool.
    ThreadPool(ThreadPoolSpec),

    /// Subprocess (strongest isolation).
    Subprocess(SubprocessSpec),
}

/// Parameters for a private thread pool.
///
/// `name_prefix` uses [`Cow<'static, str>`] so it accepts a `&'static str`
/// literal in code or an owned `String` from a config file.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ThreadPoolSpec {
    pub min_threads: usize,
    pub max_threads: usize,
    pub name_prefix: Cow<'static, str>,
}

impl ThreadPoolSpec {
    pub fn io_pool(name: impl Into<Cow<'static, str>>, max: usize) -> Self {
        Self {
            min_threads: 2,
            max_threads: max,
            name_prefix: name.into(),
        }
    }
}

/// Parameters for a subprocess execution.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SubprocessSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub restart: RestartPolicy,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum RestartPolicy {
    Never,
    MaxRetries(u32),
    Always { delay_ms: u64 },
}
