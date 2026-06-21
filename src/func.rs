/// Pure-function computation primitive.
///
/// # Physics
/// - **Memory**: stack frame. Allocated on call, destroyed on return.
/// - **Lifetime**: unobservable — the frame lives too briefly for any observer.
/// - **Connection**: only via function call (pass-by-value or pass-by-reference).
///   Cannot be attached to a BoundedBuffer or Channel.
/// - **Determinism**: by default guaranteed (same input → same output).
///   Override `determinism()` if the function uses randomness or external state.
///
/// # When to use
/// Any computation that does not need to retain state between invocations:
/// parsing, serialization, mathematical transforms, segment detection, signal computation.
///
/// # Zero-cost guarantee
/// `Func::call(input)` compiles to the same machine code as a direct `fn(input)` call.
/// The trait adds no dispatch overhead when called through a concrete type.
pub trait Func: Send + Sync + 'static {
    /// The input type, received by value or reference.
    type Input: Send + 'static;

    /// The output type, produced by value.
    type Output: Send + Sync + 'static;

    /// Human-readable name for diagnostics and topology displays.
    fn name() -> &'static str
    where
        Self: Sized;

    /// Execute the computation.
    ///
    /// # Contract
    /// - Must not access heap-persistent state outside `input`.
    /// - Must not block.
    /// - Must complete in bounded time.
    fn call(input: Self::Input) -> Self::Output;

    /// Estimated computational cost, for the deployer's scheduling decisions.
    ///
    /// Returns `Unknown` by default. Override with a measured value if available.
    fn cost_estimate() -> CostEstimate
    where
        Self: Sized,
    {
        CostEstimate::Unknown
    }

    /// Whether this function depends on external nondeterministic state
    /// (randomness, wall-clock time, network).
    ///
    /// `true` means the deployer must NOT assume deterministic replay is safe.
    fn nondeterministic() -> bool
    where
        Self: Sized,
    {
        false
    }
}

/// Rough estimate of a function's computational cost, for scheduling decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CostEstimate {
    /// Cost not measured; deployer should assume moderate.
    Unknown,
    /// ~1–10 CPU cycles (register-only ops, bit manipulation).
    Trivial,
    /// ~10–100 cycles (simple arithmetic, small loops).
    Cheap,
    /// ~100–1_000 cycles (moderate loops, hash computation).
    Moderate,
    /// ~1_000–10_000 cycles (allocation, crypto, large loops).
    Expensive,
    /// >10_000 cycles (IO, serialization, complex algorithms).
    VeryExpensive,
}

impl CostEstimate {
    pub fn is_unknown(&self) -> bool {
        matches!(self, CostEstimate::Unknown)
    }
}

// ════════════════════════════════════════════════════════════
// FuncWithScratch — reused scratch space for hot-path Funcs
// ════════════════════════════════════════════════════════════

/// A `Func` that additionally accepts a reusable scratch buffer.
///
/// # When to use
/// Any `Func` called repeatedly on a hot path (millions of invocations).
/// The `Scratch` is allocated **once** by the caller and passed to every
/// invocation. This eliminates per-call heap allocations.
///
/// # Zero-cost guarantee
/// A single `Scratch::default()` allocation at pipeline construction time.
/// Zero allocations during `call_with()` — all temporary storage reuses
/// capacity from the scratch buffer.
///
/// # Contract
/// - `call_with()` must clear the scratch before or after use so that the
///   next invocation starts clean.
/// - The caller may reuse the same scratch for multiple distinct Funcs
///   in a pipeline — each Func reads from the scratch's *output* area
///   and writes to the scratch's *input* area, but they must not conflict.
pub trait FuncWithScratch: Func {
    /// Scratch workspace, allocated once by the caller.
    /// Must implement `Default` so the pipeline can pre-allocate it.
    type Scratch: Default + Send + 'static;

    /// Execute the computation using a caller-provided scratch buffer.
    ///
    /// # Contract
    /// Same as `Func::call()`, plus:
    /// - The scratch must be in a clean state at the start of each call.
    ///   Implementations should clear or reset the scratch at *exit*.
    fn call_with(input: Self::Input, scratch: &mut Self::Scratch) -> Self::Output;
}

// ── Scratched wrapper ─────────────────────────────────────

/// A `Func` that also implements `FuncWithScratch` can be called
/// without a scratch (via the standard `Func::call` path) — the default
/// implementation creates a fresh scratch on every call.
///
/// Override this if `call()` should route through `call_with()` with
/// a stack-allocated or thread-local scratch.
pub struct Scratched<F: FuncWithScratch>(std::marker::PhantomData<F>);

impl<F: FuncWithScratch> Func for Scratched<F> {
    type Input = F::Input;
    type Output = F::Output;

    fn name() -> &'static str { F::name() }

    fn call(input: Self::Input) -> Self::Output {
        let mut scratch = F::Scratch::default();
        F::call_with(input, &mut scratch)
    }

    fn cost_estimate() -> CostEstimate { F::cost_estimate() }
    fn nondeterministic() -> bool { F::nondeterministic() }
}

// ── Pipeline composer ─────────────────────────────────────

/// A compile-time chain of `FuncWithScratch` steps that share a single
/// compound scratch buffer. The entire pipeline allocates its scratch
/// once — zero allocation during `process()`.
///
/// # Example
///
/// ```ignore
/// type MyPipeline = FuncScratchPipeline<(Parse, Scale, Format)>;
///
/// let mut scratch = <<MyPipeline as FuncWithScratch>::Scratch as Default>::default();
/// let output = MyPipeline::call_with(input, &mut scratch);
/// ```
pub struct FuncScratchPipeline<Steps>(std::marker::PhantomData<Steps>);

// ── Single-step pipeline ──────────────────────────────────

impl<A: FuncWithScratch> FuncWithScratch for FuncScratchPipeline<(A,)>
where
    A::Scratch: Default,
{
    type Scratch = A::Scratch;

    fn call_with(input: <Self as Func>::Input, scratch: &mut Self::Scratch) -> <Self as Func>::Output {
        A::call_with(input, scratch)
    }
}

impl<A: FuncWithScratch> Func for FuncScratchPipeline<(A,)>
where
    A::Scratch: Default,
{
    type Input = A::Input;
    type Output = A::Output;

    fn name() -> &'static str { A::name() }
    fn call(input: Self::Input) -> Self::Output { A::call(input) }
    fn cost_estimate() -> CostEstimate { A::cost_estimate() }
}

// ── Two-step pipeline: A ⤳ B ──────────────────────────────

impl<A, B> FuncWithScratch for FuncScratchPipeline<(A, B)>
where
    A: FuncWithScratch<Output = B::Input>,
    B: FuncWithScratch,
    A::Scratch: Default,
    B::Scratch: Default,
{
    type Scratch = (A::Scratch, B::Scratch);

    fn call_with(input: <Self as Func>::Input, scratch: &mut Self::Scratch) -> <Self as Func>::Output {
        let mid = A::call_with(input, &mut scratch.0);
        B::call_with(mid, &mut scratch.1)
    }
}

impl<A, B> Func for FuncScratchPipeline<(A, B)>
where
    A: FuncWithScratch<Output = B::Input>,
    B: FuncWithScratch,
    A::Scratch: Default,
    B::Scratch: Default,
{
    type Input = A::Input;
    type Output = B::Output;

    fn name() -> &'static str { "pipeline" }
    fn call(input: Self::Input) -> Self::Output {
        let mut s: (<A as FuncWithScratch>::Scratch, <B as FuncWithScratch>::Scratch) = Default::default();
        let mid = A::call_with(input, &mut s.0);
        B::call_with(mid, &mut s.1)
    }
}

// ── Three-step pipeline: A ⤳ B ⤳ C ───────────────────────

impl<A, B, C> FuncWithScratch for FuncScratchPipeline<(A, B, C)>
where
    A: FuncWithScratch<Output = B::Input>,
    B: FuncWithScratch<Output = C::Input>,
    C: FuncWithScratch,
    A::Scratch: Default,
    B::Scratch: Default,
    C::Scratch: Default,
{
    type Scratch = (A::Scratch, B::Scratch, C::Scratch);

    fn call_with(input: <Self as Func>::Input, scratch: &mut Self::Scratch) -> <Self as Func>::Output {
        let a = A::call_with(input, &mut scratch.0);
        let b = B::call_with(a, &mut scratch.1);
        C::call_with(b, &mut scratch.2)
    }
}

impl<A, B, C> Func for FuncScratchPipeline<(A, B, C)>
where
    A: FuncWithScratch<Output = B::Input>,
    B: FuncWithScratch<Output = C::Input>,
    C: FuncWithScratch,
    A::Scratch: Default,
    B::Scratch: Default,
    C::Scratch: Default,
{
    type Input = A::Input;
    type Output = C::Output;

    fn name() -> &'static str { "pipeline" }
    fn call(input: Self::Input) -> Self::Output {
        let mut s: (
            <A as FuncWithScratch>::Scratch,
            <B as FuncWithScratch>::Scratch,
            <C as FuncWithScratch>::Scratch,
        ) = Default::default();
        let a = A::call_with(input, &mut s.0);
        let b = B::call_with(a, &mut s.1);
        C::call_with(b, &mut s.2)
    }
}
