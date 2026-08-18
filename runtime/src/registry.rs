//! Machine type registry — maps `machine_type` strings to `RegisterFn` constructors.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;

use axiom::machine::Machine;
use axiom::port::{MachineContext, PortSchema};

// CompositeSpec is now referenced from core — composition is a structural definition capability that belongs to axiom core.
use axiom::composite::CompositeSpec;
use crate::erasure::{MachineWrapper, RunningMachine, ScratchMachine};
use crate::error::RuntimeError;

/// Machine constructor — builds `Box<dyn RunningMachine>` from `MachineContext`.
pub trait RegisterFn: Send + Sync {
    fn build(&self, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError>;

    /// Whether the machine type this registrar corresponds to guarantees implementation of [`axiom::machine::Moore`].
    ///
    /// Defaults to `false`. Only machines registered via [`Registry::register_moore`] (type-level
    /// `M: Moore` constraint) return `true`. Used during materialization to validate consistency
    /// between the deployment declaration `MachineInstance::is_moore` and the implementation (S3-2).
    fn is_moore(&self) -> bool {
        false
    }

    /// The port schema of the machine type this registrar builds.
    ///
    /// Captured at registration time (`M::port_schema()`). Used by `materialize`
    /// to assemble the schema map that deep validation
    /// ([`axiom::deploy::DynamicTopology::validate_deep_for`]) needs to check
    /// port existence, type compatibility and the FlowKind×carrier matrix
    /// before any physics is created.
    fn schema(&self) -> PortSchema;
}

struct TypedRegisterFn<M: Machine>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    fused: bool,
    /// Whether registered via `register_moore` (type-level guarantee of `M: Moore`).
    moore: bool,
    /// `M::port_schema()` — captured so the registry can feed deep validation.
    schema: PortSchema,
    _phantom: core::marker::PhantomData<M>,
}

impl<M: Machine> RegisterFn for TypedRegisterFn<M>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    fn build(&self, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError> {
        let wrapper = MachineWrapper::<M>::new(ctx, self.fused)?;
        Ok(Box::new(wrapper))
    }

    fn is_moore(&self) -> bool {
        self.moore
    }

    fn schema(&self) -> PortSchema {
        self.schema.clone()
    }
}

/// Typed fused registrar — builds [`ScratchMachine`] (allocation-free inter-stage passing after the unsafe workaround).
/// `M::Input: Pack` + `M::Output: Unpack` (automatically satisfied for FusedInline
/// single-input single-output machines via declare_ports).
struct TypedFusedRegisterFn<M: Machine>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    /// `M::port_schema()` — captured so the registry can feed deep validation.
    schema: PortSchema,
    /// Whether registered via `register_fused_moore` — the fused-path Moore contract
    /// channel. Closed the gap where fused machines could never be declared Moore
    /// (`is_moore()` always returned `false`), so a genuinely-Moore fused type was
    /// rejected with `MooreMismatch` on an honest declaration.
    moore: bool,
    _phantom: core::marker::PhantomData<M>,
}

impl<M: Machine> RegisterFn for TypedFusedRegisterFn<M>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    fn build(&self, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError> {
        Ok(Box::new(ScratchMachine::<M>::new(ctx, true)?))
    }

    fn is_moore(&self) -> bool {
        self.moore
    }

    fn schema(&self) -> PortSchema {
        self.schema.clone()
    }
}

/// Machine type registry — maps `machine_type` strings to `RegisterFn` or composite definitions.
pub struct Registry {
    builders: BTreeMap<String, Box<dyn RegisterFn>>,
    /// Composite machine_type → sub-topology + port mapping. Expanded during materialization.
    composites: BTreeMap<String, CompositeSpec>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            builders: BTreeMap::new(),
            composites: BTreeMap::new(),
        }
    }

    pub fn register<M>(&mut self, machine_type: &str)
    where
        M: Machine,
        M::Input: core::any::Any + Send,
        M::Output: core::any::Any + Send,
    {
        self.builders.insert(
            machine_type.to_string(),
            Box::new(TypedRegisterFn::<M> {
                fused: false,
                moore: false,
                schema: M::port_schema(),
                _phantom: core::marker::PhantomData,
            }),
        );
    }

    /// Register a machine with **Moore semantics** — `M: axiom::machine::Moore` guarantees at the
    /// type level that outputs depend only on the pre-update state (can break algebraic cycles in
    /// feedback loops).
    ///
    /// The only difference from [`Self::register`] is the `moore: true` flag: during materialization
    /// it validates that the deployment declaration `MachineInstance::is_moore` matches the
    /// implementation (S3-2) — only machine types registered via `register_moore` are allowed to
    /// declare Moore semantics, otherwise [`crate::error::RuntimeError::MooreMismatch`] is returned.
    pub fn register_moore<M>(&mut self, machine_type: &str)
    where
        M: Machine + axiom::machine::Moore,
        M::Input: core::any::Any + Send,
        M::Output: core::any::Any + Send,
    {
        self.builders.insert(
            machine_type.to_string(),
            Box::new(TypedRegisterFn::<M> {
                fused: false,
                moore: true,
                schema: M::port_schema(),
                _phantom: core::marker::PhantomData,
            }),
        );
    }

    /// Register a fusible machine — `M: FusedInline` guarantees at the type level that `ProcessOutput`
    /// is `SingleOutput`/`TupleOutput` (fixed output count, no `YieldMulti`), and
    /// `M::Input: Pack` + `M::Output: Unpack` (single-input single-output, automatically satisfied
    /// via declare_ports) — `materialize` can then include it in a `FusedPipeline` chain, passing
    /// between stages through a `ScratchMachine` typed single slot without boxing (unsafe workaround,
    /// zero allocation between same-type stages).
    pub fn register_fused<M>(&mut self, machine_type: &str)
    where
        M: Machine + axiom::machine::FusedInline,
        M::Input: core::any::Any + Send + axiom::portset::Pack,
        M::Output: core::any::Any + Send + axiom::portset::Unpack,
        M::ProcessOutput: axiom::machine::FusedCompatible,
    {
        self.builders.insert(
            machine_type.to_string(),
            Box::new(TypedFusedRegisterFn::<M> {
                schema: M::port_schema(),
                moore: false,
                _phantom: core::marker::PhantomData,
            }),
        );
    }

    /// Register a **fusible** machine with **Moore semantics** — `M: FusedInline + Moore`
    /// guarantees at the type level both fused pass-through and that outputs depend only
    /// on the pre-update state. The fused-path analogue of [`Self::register_moore`]: it
    /// records `moore = true` so a genuine Moore fused type can be *honestly* declared
    /// `.moore()` in the topology and pass the S3-2 materialization check — closing the
    /// gap where fused registrars returned `is_moore() == false` unconditionally.
    pub fn register_fused_moore<M>(&mut self, machine_type: &str)
    where
        M: Machine + axiom::machine::FusedInline + axiom::machine::Moore,
        M::Input: core::any::Any + Send + axiom::portset::Pack,
        M::Output: core::any::Any + Send + axiom::portset::Unpack,
        M::ProcessOutput: axiom::machine::FusedCompatible,
    {
        self.builders.insert(
            machine_type.to_string(),
            Box::new(TypedFusedRegisterFn::<M> {
                schema: M::port_schema(),
                moore: true,
                _phantom: core::marker::PhantomData,
            }),
        );
    }

    /// Register a composite Machine — sub-topology + port mapping. Expanded during materialization
    /// into namespaced sub-machines + redirected external links.
    pub fn register_composite(&mut self, machine_type: &str, spec: CompositeSpec) {
        self.composites.insert(machine_type.to_string(), spec);
    }

    /// Registered composite definitions (for materialization expansion).
    pub(crate) fn composites(&self) -> &BTreeMap<String, CompositeSpec> {
        &self.composites
    }

    pub(crate) fn build(&self, machine_type: &str, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError> {
        let builder = self.builders.get(machine_type)
            .ok_or_else(|| RuntimeError::InitFailed {
                machine: machine_type.to_string(),
                error: axiom::machine::InitError::Other(format!("type `{machine_type}` not registered")),
            })?;
        builder.build(ctx)
    }

    /// Whether the registered `machine_type` guarantees Moore semantics (for S3-2 contract validation).
    /// Unregistered types return `false` — the materialization layer rejects their Moore declaration
    /// with `MooreMismatch` (if declared).
    pub(crate) fn is_moore(&self, machine_type: &str) -> bool {
        self.builders
            .get(machine_type)
            .map(|b| b.is_moore())
            .unwrap_or(false)
    }

    /// The port schema of a registered `machine_type`, if it is a concrete
    /// machine (not a composite). `None` for unregistered types — the
    /// materialization layer reports them as unknown before building physics.
    pub(crate) fn schema(&self, machine_type: &str) -> Option<PortSchema> {
        self.builders.get(machine_type).map(|b| b.schema())
    }
}

impl Default for Registry {
    fn default() -> Self { Self::new() }
}
