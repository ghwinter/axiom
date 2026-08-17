//! **Maturity: experimental** (an extension; advanced as part of the core, not dropped).
//!
//! Hybrid System Extension — continuous dynamics + discrete state machines.
//!
//! # Theoretical foundation
//!
//! Based on Hybrid Automata (Alur, Courcoubetis, Henzinger, Ho, 1995) and
//! the Ramadge-Wonham supervisory control framework. A hybrid system mixes:
//!
//! - **Continuous evolution** — differential equations that govern how
//!   continuous state variables change over time between discrete events.
//! - **Discrete transitions** — instantaneous jumps triggered by guard
//!   conditions on the continuous state.
//!
//! # Problem this solves
//!
//! axiom's `Machine` model is purely discrete: state changes only when
//! `process()` is called with an input. But many real-world systems have
//! continuous dynamics:
//!
//! - **CPS (Cyber-Physical Systems)**: temperature, pressure, position
//! - **Finance**: continuous-time price models (geometric Brownian motion)
//! - **Robotics**: motor controllers with continuous velocity/acceleration
//! - **Game servers**: physics simulation between network ticks
//!
//! The `HybridMachine` trait extends `Machine` with a `flow` method that
//! advances continuous state by a time delta `dt`.
//!
//! # Unified model
//!
//! The hybrid state is the product of continuous and discrete components:
//!
//! ```text
//! S = S_c × S_d
//! ```
//!
//! - `S_c` — continuous state (evolves via ODEs between jumps)
//! - `S_d` — discrete state (transitions via instantaneous jumps)
//!
//! The dynamics are governed by two mechanisms:
//!
//! 1. **Flow** (continuous): `dc/dt = f(c, d)` — evolves `S_c` while `S_d`
//!    is held constant between discrete events.
//! 2. **Jump** (discrete): `(c, d) → (c', d')` — an instantaneous transition
//!    triggered when `guard(c, d)` returns a [`Jump`]. After the discrete
//!    state changes, `reset()` may update the continuous state.
//!
//! The combined transition function is:
//!
//! ```text
//! δ_hybrid: (S_c, S_d) × I × Δt → (S_c, S_d) × O
//! ```
//!
//! where `Δt` is the time elapsed since the last transition, obtained from
//! the runtime's [`TimeTick`].
//!
//! # Usage
//!
//! ```ignore
//! use axiom::hybrid::{HybridMachine, HybridState, Jump};
//! use axiom::prelude_all::*;
//!
//! struct Thermostat;
//!
//! impl HybridMachine for Thermostat {
//!     type Continuous = f64;       // temperature
//!     type DiscreteState = bool;   // heater on/off
//!
//!     fn flow(c: &f64, dt: f64, d: &bool) -> f64 {
//!         if *d {
//!             c + 0.1 * dt   // heating
//!         } else {
//!             c - 0.05 * dt  // cooling
//!         }
//!     }
//!
//!     fn guard(c: &f64, d: &bool) -> Option<Jump<bool>> {
//!         if *d && *c > 25.0 {
//!             Some(Jump::Transition(false))   // turn off heater
//!         } else if !*d && *c < 20.0 {
//!             Some(Jump::Transition(true))    // turn on heater
//!         } else {
//!             None
//!         }
//!     }
//! }
//! ```

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use crate::port::MachineContext;
use crate::time::TimeTick;

// ── Continuous state trait ──────────────────────────────────────────────────

/// A marker trait for types that can serve as continuous state.
///
/// Implementors should be types that support arithmetic operations
/// (e.g., `f64`, `Vec<f64>`, a custom struct with `+` and `*`).
/// The trait itself is empty — it exists for documentation and
/// potential future extensions (e.g., serialization for checkpointing).
pub trait ContinuousState: Send + Clone + 'static {}

impl ContinuousState for f64 {}
impl ContinuousState for f32 {}
impl ContinuousState for (f64, f64) {}
impl ContinuousState for (f64, f64, f64) {}
impl ContinuousState for Vec<f64> {}

// ── Hybrid state ────────────────────────────────────────────────────────────

/// The unified hybrid state: continuous × discrete.
///
/// This is the product `S = S_c × S_d` from the hybrid automaton model.
/// The continuous component evolves via ODEs; the discrete component
/// transitions via instantaneous jumps.
///
/// Both components are observable and can be queried at any time.
#[derive(Debug, Clone)]
pub struct HybridState<C, D> {
    /// Continuous state (evolves via `flow`).
    pub continuous: C,
    /// Discrete state (transitions via `Jump`).
    pub discrete: D,
}

impl<C, D> HybridState<C, D> {
    /// Create a new hybrid state from its continuous and discrete components.
    pub fn new(continuous: C, discrete: D) -> Self {
        Self { continuous, discrete }
    }

    /// Borrow the continuous component.
    pub fn continuous(&self) -> &C {
        &self.continuous
    }

    /// Borrow the discrete component.
    pub fn discrete(&self) -> &D {
        &self.discrete
    }
}

// ── Jump (discrete transition) ──────────────────────────────────────────────

/// A discrete jump triggered by a guard condition.
///
/// When [`HybridMachine::guard()`] returns `Some(jump)`, the runtime applies
/// the jump to the discrete state before the next `process()` call.
///
/// # Variants
///
/// - `Transition` — change the discrete state. `reset()` is invoked to
///   optionally update the continuous state.
/// - `Reset` — change the discrete state and invoke `reset()`. This is
///   semantically identical to `Transition` but signals intent: the
///   continuous state should be reinitialised. The actual reset value is
///   computed by the machine's `reset()` implementation, not hardcoded in
///   the jump — this keeps the continuous type generic.
/// - `Emit` — produce an output without changing state.
#[derive(Debug, Clone, PartialEq)]
pub enum Jump<D> {
    /// Transition the discrete state to `new_state`.
    ///
    /// After the transition, `reset()` is called so the machine can
    /// update its continuous state if needed.
    Transition(D),

    /// Transition the discrete state and signal that the continuous state
    /// should be reinitialised via `reset()`.
    ///
    /// Unlike a plain `Transition`, this variant documents intent: the
    /// continuous state is no longer valid and must be recomputed. The
    /// actual reset value is determined by the machine's `reset()`
    /// implementation, preserving full type generality over `Continuous`.
    Reset {
        /// The new discrete state after the jump.
        new_discrete: D,
    },

    /// Emit an output string without changing discrete or continuous state.
    ///
    /// The runtime is responsible for delivering this to the appropriate
    /// output port.
    Emit(String),
}

// ── Hybrid machine trait ────────────────────────────────────────────────────

/// A Machine with continuous dynamics.
///
/// Extends the discrete `Machine` model with:
/// - Continuous state `Continuous` that evolves via ODEs (`flow`)
/// - Guard conditions that trigger discrete transitions (`guard`)
/// - Reset actions that reinitialise continuous state (`reset`)
///
/// The discrete part of the machine still implements `Machine` for
/// port-based communication. The hybrid part adds time-driven evolution.
///
/// # Semantics
///
/// Between discrete `process()` calls, the runtime calls `flow()` to
/// advance the continuous state by `dt` seconds. After each flow step,
/// `guard()` is evaluated; if it returns a [`Jump`], the jump is queued
/// and applied before the next `process()` call.
pub trait HybridMachine: Send + Sync + 'static {
    /// The continuous state type (e.g., `f64` for temperature).
    type Continuous: ContinuousState;

    /// The discrete state type (the "mode" of the hybrid system).
    type DiscreteState: Send + Clone + 'static;

    /// ODE right-hand side: `dc/dt = f(c, d)`.
    ///
    /// Evolves the continuous state by `dt` seconds while the discrete
    /// state `d` is held constant. Called by the runtime between discrete
    /// `process()` calls.
    ///
    /// `dt` is in seconds (full precision, derived from `TimeTick`).
    fn flow(
        c: &Self::Continuous,
        dt: f64,
        d: &Self::DiscreteState,
    ) -> Self::Continuous;

    /// Guard condition: check whether continuous state has crossed a
    /// threshold that should trigger a discrete jump.
    ///
    /// Returns `None` if no jump is needed, or `Some(jump)` if a
    /// discrete transition should fire.
    fn guard(
        c: &Self::Continuous,
        d: &Self::DiscreteState,
    ) -> Option<Jump<Self::DiscreteState>>;

    /// Reset action invoked after a [`Jump::Transition`] or [`Jump::Reset`].
    ///
    /// Called after the discrete state has been updated from `old_d` to
    /// `new_d`. The machine may reinitialise the continuous state `c`
    /// based on the transition that occurred.
    ///
    /// Default implementation: no-op (continuous state is preserved
    /// across the jump).
    fn reset(
        c: &mut Self::Continuous,
        _old_d: &Self::DiscreteState,
        _new_d: &Self::DiscreteState,
    ) {
        let _ = c;
    }
}

// ── Hybrid driver ───────────────────────────────────────────────────────────

/// A driver that advances a hybrid machine's continuous state over time.
///
/// This is a utility for runtimes that need to step the continuous dynamics
/// between discrete `process()` calls. It does NOT call `process()` —
/// that remains the responsibility of the regular runtime.
///
/// # Time handling
///
/// The driver uses [`TimeTick`] for full-precision nanosecond time. The
/// runtime sets the current time via [`step_to_tick`](Self::step_to_tick)
/// or [`step_with_context`](Self::step_with_context), and the driver
/// computes `dt` automatically from the elapsed time since the last step.
///
/// # Thread safety
///
/// `HybridDriver` holds mutable state (`&mut`) and is intended for
/// single-threaded stepping. For multi-threaded use, wrap in `Mutex`.
pub struct HybridDriver<H: HybridMachine> {
    state: HybridState<H::Continuous, H::DiscreteState>,
    last_tick: TimeTick,
    /// Whether this is the first step (dt = 0 on first call).
    initialised: bool,
    /// Pending discrete jumps that haven't been applied yet.
    pending_jumps: Vec<Jump<H::DiscreteState>>,
}

impl<H: HybridMachine> HybridDriver<H> {
    /// Create a new driver with initial continuous and discrete state.
    ///
    /// The driver starts with `last_tick = 0`; the first call to
    /// [`step_to_tick`](Self::step_to_tick) or
    /// [`step_with_context`](Self::step_with_context) will set the
    /// baseline time without evolving the continuous state (dt = 0).
    pub fn new(continuous: H::Continuous, discrete: H::DiscreteState) -> Self {
        Self {
            state: HybridState::new(continuous, discrete),
            last_tick: TimeTick::from_nanos(0),
            initialised: false,
            pending_jumps: Vec::new(),
        }
    }

    /// Create a new driver from a unified [`HybridState`].
    pub fn from_state(state: HybridState<H::Continuous, H::DiscreteState>) -> Self {
        Self::new(state.continuous, state.discrete)
    }

    /// Borrow the full hybrid state.
    pub fn state(&self) -> &HybridState<H::Continuous, H::DiscreteState> {
        &self.state
    }

    /// Borrow the continuous component.
    pub fn continuous(&self) -> &H::Continuous {
        &self.state.continuous
    }

    /// Borrow the discrete component.
    pub fn discrete(&self) -> &H::DiscreteState {
        &self.state.discrete
    }

    /// Advance the continuous dynamics by `dt` seconds.
    ///
    /// After evolving, checks the guard condition. If the guard fires,
    /// the discrete jump is queued for later application via
    /// [`apply_pending_jumps`](Self::apply_pending_jumps).
    ///
    /// This method does not consult the clock — use
    /// [`step_to_tick`](Self::step_to_tick) for clock-driven stepping.
    pub fn step(&mut self, dt_seconds: f64) {
        // Evolve continuous state via the ODE flow.
        self.state.continuous = H::flow(&self.state.continuous, dt_seconds, &self.state.discrete);

        // Check guard after the flow step.
        if let Some(jump) = H::guard(&self.state.continuous, &self.state.discrete) {
            self.pending_jumps.push(jump);
        }
    }

    /// Advance to a specific [`TimeTick`].
    ///
    /// Computes the elapsed time since the last call and steps the
    /// continuous dynamics. On the first call, `dt` is 0 (baseline set
    /// without evolution).
    ///
    /// This is the preferred method for clock-driven runtimes.
    pub fn step_to_tick(&mut self, tick: TimeTick) {
        if !self.initialised {
            self.last_tick = tick;
            self.initialised = true;
            return;
        }

        let dt_ns = tick.ns.saturating_sub(self.last_tick.ns);
        self.last_tick = tick;
        let dt_seconds = dt_ns as f64 / 1_000_000_000.0;
        self.step(dt_seconds);
    }

    /// Advance using the time from a [`MachineContext`].
    ///
    /// Reads `ctx.time_tick()` and delegates to
    /// [`step_to_tick`](Self::step_to_tick). This is the most convenient
    /// method for runtimes that already provide a `MachineContext` to
    /// each machine.
    pub fn step_with_context(&mut self, ctx: &MachineContext) {
        self.step_to_tick(ctx.time_tick());
    }

    /// Apply all pending discrete jumps.
    ///
    /// Returns the jumps that were applied (in order).
    ///
    /// For [`Jump::Transition`] and [`Jump::Reset`], the old discrete state
    /// is replaced and `reset()` is called to update the continuous state.
    /// For [`Jump::Emit`], no state change occurs.
    pub fn apply_pending_jumps(&mut self) -> Vec<Jump<H::DiscreteState>> {
        let jumps = core::mem::take(&mut self.pending_jumps);
        for jump in &jumps {
            match jump {
                Jump::Transition(new_d) => {
                    let old_d = core::mem::replace(&mut self.state.discrete, new_d.clone());
                    H::reset(&mut self.state.continuous, &old_d, &self.state.discrete);
                }
                Jump::Reset { new_discrete } => {
                    let old_d = core::mem::replace(&mut self.state.discrete, new_discrete.clone());
                    H::reset(&mut self.state.continuous, &old_d, &self.state.discrete);
                }
                Jump::Emit(_) => {
                    // No state change; the runtime will handle the output.
                }
            }
        }
        jumps
    }

    /// Whether any discrete jumps are pending.
    pub fn has_pending_jumps(&self) -> bool {
        !self.pending_jumps.is_empty()
    }

    /// The number of pending jumps.
    pub fn pending_count(&self) -> usize {
        self.pending_jumps.len()
    }
}

impl<H: HybridMachine> core::fmt::Debug for HybridDriver<H>
where
    H::Continuous: core::fmt::Debug,
    H::DiscreteState: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HybridDriver")
            .field("state", &self.state)
            .field("last_tick", &self.last_tick)
            .field("initialised", &self.initialised)
            .field("pending_jumps", &self.pending_jumps.len())
            .finish()
    }
}

