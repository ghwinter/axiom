/// Resource classes for lifecycle-aware resource tracking.
///
/// Every `Machine` consumes resources. Some are reclaimable when the machine
/// stops; others are permanent. This module codifies the distinction.

// ── Resource class ────────────────────────────────────────────────────────────

/// Classification of a resource by its reclaimability.
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

/// Physical resource requirements for a `Machine` instance.
///
/// This is specified by the **deployer** in the `DeploySpec`, not by the
/// machine author. The same machine type can have different physical specs
/// in different deployments (backtest vs. production).
#[derive(Debug, Clone)]
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
}

impl Default for MachinePhysicalSpec {
    fn default() -> Self {
        Self {
            execution: ExecutionHint::Async,
            state_heap_bytes: 4096,
            cache_line_align: false,
            deterministic: false,
            max_cleanup_latency_us: 10_000,
        }
    }
}

// ── Execution hints ───────────────────────────────────────────────────────────

/// Execution strategy for a `Machine` instance.
///
/// Chosen by the deployer. The same machine type can be deployed with
/// different execution hints in different contexts.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct ThreadPoolSpec {
    pub min_threads: usize,
    pub max_threads: usize,
    pub name_prefix: &'static str,
}

impl ThreadPoolSpec {
    pub fn io_pool(name: &'static str, max: usize) -> Self {
        Self {
            min_threads: 2,
            max_threads: max,
            name_prefix: name,
        }
    }
}

/// Parameters for a subprocess execution.
#[derive(Debug, Clone)]
pub struct SubprocessSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub restart: RestartPolicy,
}

#[derive(Debug, Clone)]
pub enum RestartPolicy {
    Never,
    MaxRetries(u32),
    Always { delay_ms: u64 },
}
