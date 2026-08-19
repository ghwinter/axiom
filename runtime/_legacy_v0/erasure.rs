//! Type erasure layer — `RunningMachine` trait + `MachineWrapper` adapter.
//!
//! The runtime holds `Box<dyn RunningMachine>` and does not need to know the concrete `M` type.
//! `M::Input` is restored via `Box<dyn Any>` downcast; `M::Output` extracts the port name via
//! `HasPortInfo::port_name()` and type-erases the value with `into_any()`.

use alloc::boxed::Box;
use alloc::vec::Vec;

use axiom::machine::{Machine, MachineHandle, Init, Running};
use axiom::port::MachineContext;
use axiom::portset::HasPortInfo;

use crate::error::RuntimeError;

/// Free a raw-pointer allocation produced by [`crate::typed_slot::take_input`]
/// when no output is written back (Idle/Done/early-return).
///
/// # Safety
///
/// `raw_ptr` must come from `take_input::<Raw>` (i.e. `Box::<Raw>::into_raw`). The
/// content was bit-copied out and moved into the machine — the memory is
/// uninitialized and must NOT be dropped; the allocation is freed with the exact
/// `Layout::new::<Raw>()`, matching the intended dealloc.
fn free_raw<Raw: 'static>(raw_ptr: *mut Raw) {
    // SAFETY: `raw_ptr` is the allocation of a `Box<Raw>` (see take_input); the
    // content is uninitialized (bit-copied out), so `dealloc` never drops anything.
    unsafe {
        alloc::alloc::dealloc(raw_ptr as *mut u8, core::alloc::Layout::new::<Raw>());
    }
}

/// A running Machine after type erasure — the runtime holds `Box<dyn RunningMachine>`,
/// so it does not need to know the concrete `M` type.
///
/// `process_boxed` receives `Box<dyn Any + Send>` (a type-erased input) and returns
/// `ProcessResult` (containing the port name and the erased output value).
/// The port name is extracted from the output value by `HasPortInfo::port_name()`; the runtime
/// uses it to match the source port of a `LinkSpec` and find the target machine and port.
pub trait RunningMachine: Send {
    fn name(&self) -> &str;
    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult;
    /// Inject a routed payload by port ID: the ID is restored to a port name via `in_port_names`,
    /// and `HasPortInfo::from_port_name` builds this machine's input variant and processes it.
    /// `Idle` means the port did not match. ID-based injection eliminates string matching and
    /// boxing on the hot path.
    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult;
    /// Typed single-slot processing (the allocation-free inter-stage protocol after the unsafe
    /// workaround): take the raw value from `slot` ([`take_input`] bit copy, zero allocation),
    /// build the input with `Pack`, process, `Unpack` the raw value, and write it back to the same
    /// slot via [`put_output`] (same type = zero allocation / cross-type = re-box).
    /// Only [`ScratchMachine`] (`M::Input: Pack` + `M::Output: Unpack` single-input single-output
    /// machines) overrides this; other machines return `Idle`.
    fn process_scratch(
        &mut self,
        _port_id: u16,
        _slot: &mut Option<Box<dyn core::any::Any + Send>>,
    ) -> ScratchResult {
        ScratchResult::Idle
    }
    fn is_done(&self) -> bool;
    fn port_schema(&self) -> &axiom::port::PortSchema;
    /// Whether the machine can enter a fused pipeline (machines registered via
    /// `register_fused::<M: FusedInline>()` return `true`). `materialize` uses this flag to
    /// recognize fusible Inline chains and replace them with a `FusedPipeline` — eliminating the
    /// per-hop route lookup and queue overhead.
    fn is_fused_compatible(&self) -> bool;
    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError>;
}

/// The result of a `process` call — a simplified `ProcessOutput` used for routing after type erasure.
#[derive(Debug)]
pub enum ProcessResult {
    Idle,
    Done,
    Yield { port: &'static str, value: Box<dyn core::any::Any + Send> },
    YieldMulti { outputs: Vec<(&'static str, Box<dyn core::any::Any + Send>)> },
}

/// Result of typed single-slot processing (the allocation-free inter-stage protocol after the
/// unsafe workaround).
///
/// [`RunningMachine::process_scratch`] takes the raw value from the caller-supplied [`TypedSlot`],
/// processes it, and writes it back to the same slot via [`recycle`] (same type = zero allocation /
/// cross-type = re-box). The `Yield` value is **implicit in the slot** (unboxed); the caller moves
/// it directly.
#[derive(Debug)]
pub enum ScratchResult {
    Idle,
    Done,
    /// Single output: the output value is already in the slot, `port` is the output port name.
    Yield(&'static str),
}

/// Wraps a concrete `MachineHandle<M, Running>` into a `Box<dyn RunningMachine>`.
///
/// `M::Input` is restored via `Box<dyn Any>` downcast.
/// `M::Output` extracts the port name via `HasPortInfo::port_name()`,
/// and type-erases the value with `HasPortInfo::into_any()`.
pub(crate) struct MachineWrapper<M: Machine> {
    handle: Option<MachineHandle<M, Running>>,
    done: bool,
    fused: bool,
    schema: axiom::port::PortSchema,
    /// Input port name table (in schema.inputs() order) — used to restore port names in inject(port_id).
    in_names: Vec<&'static str>,
}

impl<M: Machine> MachineWrapper<M>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    pub(crate) fn new(ctx: MachineContext, fused: bool) -> Result<Self, RuntimeError> {
        let handle = MachineHandle::<M, Init>::new(ctx)
            .map_err(|e| RuntimeError::InitFailed {
                machine: M::name().to_string(),
                error: e,
            })?
            .start();
        let schema = M::port_schema();
        let in_names: Vec<&'static str> = schema.inputs().map(|p| p.name).collect();
        Ok(Self {
            handle: Some(handle),
            done: false,
            fused,
            schema,
            in_names,
        })
    }

    /// Unified tail: process a concrete input → type-erased output. Shared by inject and
    /// process_boxed — inject skips boxing + downcast (P0: eliminates redundant heap allocation
    /// on the dynamic path).
    fn process_input(&mut self, input: M::Input) -> ProcessResult {
        let handle = match self.handle.as_mut() {
            Some(h) => h,
            None => return ProcessResult::Idle,
        };

        let output = handle.process(input);
        let unified = <M::ProcessOutput as axiom::machine::MachineOutput<M::Output>>::into_process_output(output);
        match unified {
            axiom::machine::ProcessOutput::Yield(o) => {
                let port = HasPortInfo::port_name(&o);
                let value = HasPortInfo::into_any(o);
                ProcessResult::Yield { port, value }
            }
            axiom::machine::ProcessOutput::YieldMulti(outs) => {
                let mapped = outs.into_iter().map(|o| {
                    let port = HasPortInfo::port_name(&o);
                    let value = HasPortInfo::into_any(o);
                    (port, value)
                }).collect();
                ProcessResult::YieldMulti { outputs: mapped }
            }
            axiom::machine::ProcessOutput::Idle => ProcessResult::Idle,
            axiom::machine::ProcessOutput::Done => {
                self.done = true;
                ProcessResult::Done
            }
        }
    }
}

impl<M: Machine> RunningMachine for MachineWrapper<M>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    fn name(&self) -> &str {
        self.handle.as_ref().map(|h| h.context().name()).unwrap_or(M::name())
    }

    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult {
        let input: M::Input = match input.downcast::<M::Input>() {
            Ok(b) => *b,
            Err(_) => return ProcessResult::Idle,
        };
        self.process_input(input)
    }

    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        // ID → port name (&'static str, schema.inputs() order); build the input variant.
        let Some(port) = self.in_names.get(port_id as usize).copied() else {
            return ProcessResult::Idle;
        };
        let Some(input) = <M::Input as HasPortInfo>::from_port_name(port, payload) else {
            return ProcessResult::Idle;
        };
        self.process_input(input)
    }

    fn is_done(&self) -> bool { self.done }

    fn is_fused_compatible(&self) -> bool { self.fused }

    fn port_schema(&self) -> &axiom::port::PortSchema { &self.schema }

    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError> {
        let inner = *self;
        if let Some(handle) = inner.handle {
            let stopped = handle.stop().finish();
            stopped.cleanup().map_err(|_e| RuntimeError::CleanupFailed {
                machine: M::name().to_string(),
            })?;
        }
        Ok(())
    }
}

/// Typed single-slot stage (allocation-free inter-stage passing after the unsafe workaround:
/// the inter-stage machine of `FusedPipeline`).
///
/// Wraps [`MachineWrapper`], implementing full forwarding of [`RunningMachine`] plus a **typed
/// override** of `process_scratch`: the input is built via `Pack::pack(raw value)` (zero allocation,
/// no Box consumption from `from_port_name`), the output raw value is extracted via
/// `Unpack::unpack` (zero allocation), and then written back to the same slot via [`recycle`] —
/// **same-type inter-stage (e.g. `Step: i32→i32`) is zero allocation throughout**, with only the
/// external input allocating once.
///
/// Built only by `register_fused::<M>` (`M::Input: Pack` + `M::Output: Unpack`, FusedInline
/// single-input single-output machines) — multi-input/multi-output machines do not enter the
/// fusion chain.
pub(crate) struct ScratchMachine<M: Machine>
where
    M::Input: axiom::portset::Pack,
    M::Output: axiom::portset::Unpack,
{
    inner: MachineWrapper<M>,
}

impl<M: Machine> ScratchMachine<M>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    pub(crate) fn new(ctx: MachineContext, fused: bool) -> Result<Self, RuntimeError> {
        Ok(Self { inner: MachineWrapper::<M>::new(ctx, fused)? })
    }

    /// Typed single-slot processing: raw value Box → build Input via `Pack` → process → `Unpack`
    /// raw value → write back (same type = zero allocation / cross-type = re-box).
    fn process_scratch_typed(
        &mut self,
        port_id: u16,
        slot: &mut Option<Box<dyn core::any::Any + Send>>,
    ) -> ScratchResult {
        // Port existence check (the inter-stage protocol only serves single-input machines).
        let _port = match self.inner.in_names.get(port_id as usize) {
            Some(p) => *p,
            None => return ScratchResult::Idle,
        };
        let Some(b) = slot.take() else {
            return ScratchResult::Idle;
        };
        // Take the input raw value (the unsafe encapsulation point: bit copy + preserve allocation).
        // Invariant: on EVERY exit after this point the `raw_ptr` allocation must be either
        // written back via `put_output` (Yield) or freed (Idle/Done/early-return) — otherwise the
        // inter-stage Box (whose slot was taken) leaks one heap allocation per message.
        let (raw_in, raw_ptr) = match crate::typed_slot::take_input::<<M::Input as axiom::portset::Pack>::Raw>(b) {
            Ok(pair) => pair,
            Err(b) => {
                *slot = Some(b);
                return ScratchResult::Idle;
            }
        };
        let input = <M::Input as axiom::portset::Pack>::pack(raw_in);
        let Some(handle) = self.inner.handle.as_mut() else {
            // Machine already ended: no output is possible; free the input allocation.
            free_raw::<<M::Input as axiom::portset::Pack>::Raw>(raw_ptr);
            return ScratchResult::Idle;
        };
        let output = handle.process(input);
        let unified = <M::ProcessOutput as axiom::machine::MachineOutput<M::Output>>::into_process_output(output);
        match unified {
            axiom::machine::ProcessOutput::Yield(o) => {
                let port = HasPortInfo::port_name(&o);
                let raw_out = <M::Output as axiom::portset::Unpack>::unpack(o);
                // Write back: same type (equal TypeId) → allocation reuse (zero allocation);
                // cross-type → free the old allocation + re-box (one allocation at the transition point).
                let boxed = crate::typed_slot::put_output::<
                    <M::Input as axiom::portset::Pack>::Raw,
                    <M::Output as axiom::portset::Unpack>::Raw,
                >(raw_ptr, raw_out);
                *slot = Some(boxed);
                ScratchResult::Yield(port)
            }
            // `M::Output: Unpack` is single-valued, so a fused stage cannot reach YieldMulti;
            // this arm exists so a future widened `FusedCompatible` cannot silently drop outputs.
            // The input allocation is freed, and Idle reports "no single output" to the driver.
            axiom::machine::ProcessOutput::YieldMulti(_) => {
                free_raw::<<M::Input as axiom::portset::Pack>::Raw>(raw_ptr);
                ScratchResult::Idle
            }
            axiom::machine::ProcessOutput::Idle => {
                // Input consumed, no output: free the input allocation.
                free_raw::<<M::Input as axiom::portset::Pack>::Raw>(raw_ptr);
                ScratchResult::Idle
            }
            axiom::machine::ProcessOutput::Done => {
                self.inner.done = true;
                free_raw::<<M::Input as axiom::portset::Pack>::Raw>(raw_ptr);
                ScratchResult::Done
            }
        }
    }
}

impl<M: Machine> RunningMachine for ScratchMachine<M>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult {
        self.inner.process_boxed(input)
    }

    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        self.inner.inject(port_id, payload)
    }

    fn process_scratch(&mut self, port_id: u16, slot: &mut Option<Box<dyn core::any::Any + Send>>) -> ScratchResult {
        self.process_scratch_typed(port_id, slot)
    }

    fn is_done(&self) -> bool {
        self.inner.is_done()
    }

    fn port_schema(&self) -> &axiom::port::PortSchema {
        self.inner.port_schema()
    }

    fn is_fused_compatible(&self) -> bool {
        true
    }

    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError> {
        let inner = self.inner;
        Box::new(inner).cleanup()
    }
}
