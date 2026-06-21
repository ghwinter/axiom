# axiom 哲学基础

> **阅读建议：** 本文档讨论 axiom 的设计哲学——它不是什么、为什么存在、以及它试图解决什么问题。如果你更关心具体如何使用，请回到 README。

---

## Abstraction vs physics

Every abstraction marks the undifferentiated physical process of memory reads and writes with semantic labels: `PortDir::In`, `"control"`, `"data"`. The physics knows none of these — the CPU only loads from addresses and stores to addresses. But labels make code readable, systems reason-able, and errors localizable.

axiom does not try to eliminate labels. axiom ensures labels do not interfere with physical optimization, while keeping topology explicit and verifiable.

---

## Control is data

At the physical level, "Controller sends command to Sensor" and "Sensor sends reading to Controller" are the same operation: one thread writes to a memory address, another thread reads from it.

The distinction exists only in how the receiving Machine's `process()` interprets the value. A safety stop flag (`AtomicBool`) and a new sampling interval (`mpsc::channel<u64>`) use the same physical mechanism, but one triggers a shutdown and the other changes a configuration parameter.

**There is no "control" in the physical layer. Control is interpreted by the receiving end's process().**

Consequently, the IO-Object model is minimal:

```
IO-Object = (S, I, O, δ)
```

There is no separate `Observe` type. Observation data is just `Output` flowing through ports labelled `FlowKind::Observe`. There is no separate `Control` type. Control signals are just `Input` flowing through ports labelled `FlowKind::Control`.

Both are **port annotations**, not **type parameters**.

---

## Module boundaries are conventions, not walls

A boundary between two modules is where one module's code stops having direct access to another module's State fields. In axiom this is enforced by Rust's module system: each Machine's State is a private struct, only accessible by its own `process()`.

But this boundary is a convention, not a physical requirement. At the hardware level, nothing prevents a thread from writing to arbitrary memory addresses. The convention exists to make the system independently reason-able — each Machine can be understood in isolation.

If the convention is consistently followed, the system is maintainable. If it is broken (e.g., via `unsafe` shared state), the system is still physically valid but no longer locally reason-able.

**axiom is designed for reasoning reliability, not physical possibility.**

---

## Positioning: a mapping layer

axiom is not a pure abstraction layer and not an implementation framework. It is a **mapping layer** — it sits between the two.

```
Application         (betarc, server, firmware)
     ▲                    │
     │                    │ writes Machine / Func
     ▼                    ▼
axiom            ←  mapping layer
  Func / Machine         defines computation units
  PortSchema / LinkSpec  defines topology
  ExecutionHint /        defines physical resource interfaces
    PhysicalSpec
  DeploySpec             maps abstract → physical
     ▲                    │
     │                    │ implements scheduling, threading, channels
     ▼                    ▼
Runtime adapters  (axiom_tokio, axiom_rayon, axiom_linear)
     ▲
     │
     ▼
OS / Hardware
```

A pure abstraction layer says "what to do" and knows nothing about the physical layer. A framework says "how to do it" and owns the physical layer. axiom says **both in the same type system, but the upper layer does not depend on any specific implementation of the lower layer.**

`Machine::process()` is pure abstract. `MachinePhysicalSpec::execution` is a physical declaration. They live in the same trait. The application author writes `process()` without knowing which runtime will drive it. The deployer writes `DeploySpec` to map each machine to a physical execution strategy, without changing the machine's code.

This is not abstraction for abstraction's sake. It is **co-expression of intent and resource** — the machine declares what it needs, the deployer provides what the machine gets, and the two are checked for consistency at link/deploy time.

---

## What axiom is not

- Not a runtime (no tokio, no executor, no event loop)
- Not a framework (no Application trait, no main() wrapper)
- Not a trading engine (no betarc semantics)
- Not a pure abstraction layer — it also defines the physical interface
- Not a replacement for business logic — it only guarantees structural correctness

---

## Why "axiom"

An axiom is a self-evident truth that serves as a foundation. `Func` and `Machine` are the axioms of computation organization. Everything else is derived.
