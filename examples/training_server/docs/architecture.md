# Training Server — Architecture Deep Dive

> This document details the internal architecture of the concurrent neural network training server built on axiom. For a quick start, see the parent `README.md`.

---

## 1. Graph-Theoretic Topology Analysis

### 1.1 Deployment Graph

The system forms a labeled directed multigraph $\Sigma = (V, E, \ell)$:

```
V = { DataLoader, Batcher, Trainer, Evaluator, Checkpointer, Observer }

E = {
    (DataLoader, sample)      → (Batcher,    sample)      : Channel { capacity: 128 }
    (DataLoader, stats)       → (Observer,   stats)       : Channel { capacity: 256, drop: true }
    (Batcher,    batch)       → (Trainer,    batch)       : Channel { capacity: 128 }
    (Batcher,    stats)       → (Observer,   stats)       : Channel { capacity: 256, drop: true }
    (Trainer,    loss)        → (Observer,   loss)        : Channel { capacity: 256, drop: true }
    (Trainer,    model_delta) → (Evaluator,  model_delta) : Channel { capacity: 128 }
    (Trainer,    model_delta) → (Checkpointer,model_delta): Channel { capacity: 128 }
    (Trainer,    stats)       → (Observer,   stats)       : Channel { capacity: 256, drop: true }
    (Evaluator,  metrics)     → (Checkpointer,metrics)    : Channel { capacity: 128 }
    (Evaluator,  metrics)     → (Observer,   metrics)     : Channel { capacity: 256, drop: true }
    (Evaluator,  stats)       → (Observer,   stats)       : Channel { capacity: 256, drop: true }
    (Checkpointer, stats)     → (Observer,   stats)       : Channel { capacity: 256, drop: true }
}
```

### 1.2 Graph Properties

| Property | Value | Implication |
|----------|-------|-------------|
| **Vertices** | 6 | All Machine instances |
| **Edges** | 12 | Typed channels (not Inline — cross-thread) |
| **Max indegree** | 3 (Trainer → Observer: loss, model_delta → Checkpointer, stats) |
| **Max outdegree** | 3 (Trainer → Observer: loss, model_delta → 2 targets, stats → Observer) |
| **Cycles** | None | DAG: DataLoader → Batcher → Trainer → Evaluator/Checkpointer → Observer |
| **SCCs** | 6 (each vertex its own SCC) | No feedback loops — strictly feedforward pipeline |
| **Fan-out** | Trainer: model_delta → {Evaluator, Checkpointer} | Port-variant–based routing |
| **Fan-in** | Observer: stats[] × 5 + loss + metrics | 7 input variants, 1 Machine |
| **Path length** | max 4 (DataLoader → Batcher → Trainer → Evaluator → Checkpointer) |
| **SPOF** | Observer | All observability flows through it; no redundancy |

### 1.3 Static Analysis Verification

The current `DeploySpec::validate()` confirms:
- All 6 machine names exist in edge references
- All 12 port names match their respective Machine's `port_schema()`
- No cycles exist (DAG property)

What is **not yet checked** (engineering patch 7.5.5):
- TypeId compatibility between connected ports
- Inline edge acyclicity (not applicable — all edges are Channel)
- Edge degree constraints

---

## 2. Port Interface Sets

Each Machine declares its interface set $\Gamma = \{p_1, p_2, \ldots\}$ as a port enum. The `declare_ports!` macro generates the enum, `HasPortInfo` impl, `PortSet` impl, and `PortSchema` — all from a single declaration.

### 2.1 DataLoader

```rust
declare_ports! {
    pub struct DataLoaderPorts {
        input type DataLoaderInput {
            ctrl[Control] => ControlSignal,
            tick[Data] => u64,
        }
        output type DataLoaderOutput {
            sample[Data] => Sample,
            stats[Observe] => ModuleStats,
        }
    }
}
```

| Port | Direction | FlowKind | Type | Role |
|------|-----------|----------|------|------|
| `ctrl` | In | Control | `ControlSignal` | Start/pause/resume/stop |
| `tick` | In | Data | `u64` | Clock tick — one tick = one sample |
| `sample` | Out | Data | `Sample` | Generated synthetic data point |
| `stats` | Out | Observe | `ModuleStats` | Performance counters for Observer |

**process() logic:**
```
DataLoaderInput::ctrl(signal) → match signal { Start|Resume => running = true, Stop|Pause => running = false }
DataLoaderInput::tick(_)      → if running: cursor++ / emit Sample / if exhausted: Done
```

### 2.2 Batcher

```rust
declare_ports! {
    pub struct BatcherPorts {
        input type BatcherInput {
            sample[Data] => Sample,
        }
        output type BatcherOutput {
            batch[Data] => Batch,
            stats[Observe] => ModuleStats,
        }
    }
}
```

| Port | Direction | FlowKind | Type | Role |
|------|-----------|----------|------|------|
| `sample` | In | Data | `Sample` | Individual sample from DataLoader |
| `batch` | Out | Data | `Batch` | Fixed-size batch (default 32) |
| `stats` | Out | Observe | `ModuleStats` | Performance counters |

**process() logic:**
```
Sample accumulates in buffer; when buffer.len() >= batch_size → emit Batch + clear buffer
Else → Idle
```

### 2.3 Trainer

```rust
declare_ports! {
    pub struct TrainerPorts {
        input type TrainerInput {
            batch[Data] => Batch,
            ctrl[Control] => ControlSignal,
        }
        output type TrainerOutput {
            loss[Data] => Loss,
            model_delta[Data] => ModelDelta,
            stats[Observe] => ModuleStats,
        }
    }
}
```

| Port | Direction | FlowKind | Type | Role |
|------|-----------|----------|------|------|
| `batch` | In | Data | `Batch` | Training batch from Batcher |
| `ctrl` | In | Control | `ControlSignal` | Start/pause control |
| `loss` | Out | Data | `Loss` | MSE loss for this batch |
| `model_delta` | Out | Data | `ModelDelta` | Weights after SGD update |
| `stats` | Out | Observe | `ModuleStats` | Performance counters |

**process() logic (per batch):**
1. Rayon parallel forward pass (read-only) → predictions
2. MSE loss computation
3. Serial backward pass → gradients
4. SGD weight update (3 layers)
5. YieldMulti(loss, model_delta, stats)

### 2.4 Evaluator

```rust
declare_ports! {
    pub struct EvaluatorPorts {
        input type EvaluatorInput {
            model_delta[Data] => ModelDelta,
            ctrl[Control] => ControlSignal,
        }
        output type EvaluatorOutput {
            metrics[Data] => Metrics,
            stats[Observe] => ModuleStats,
        }
    }
}
```

| Port | Direction | FlowKind | Type | Role |
|------|-----------|----------|------|------|
| `model_delta` | In | Data | `ModelDelta` | Weights from Trainer |
| `ctrl` | In | Control | `ControlSignal` | Start signal |
| `metrics` | Out | Data | `Metrics` | Train loss + eval loss + MAE |
| `stats` | Out | Observe | `ModuleStats` | Performance counters |

**process() logic:**
1. Extract weights from model_delta
2. Forward pass on held-out validation set (20% of data)
3. Compute MSE + MAE
4. YieldMulti(metrics, stats)

### 2.5 Checkpointer

```rust
declare_ports! {
    pub struct CheckpointerPorts {
        input type CheckpointerInput {
            model_delta[Data] => ModelDelta,
            metrics[Data] => Metrics,
        }
        output type CheckpointerOutput {
            stats[Observe] => ModuleStats,
        }
    }
}
```

| Port | Direction | FlowKind | Type | Role |
|------|-----------|----------|------|------|
| `model_delta` | In | Data | `ModelDelta` | Weights to persist (bincode → model.bin) |
| `metrics` | In | Data | `Metrics` | Metrics to persist (JSONL → metrics.jsonl) |
| `stats` | Out | Observe | `ModuleStats` | Performance counters |

### 2.6 Observer

```rust
declare_ports! {
    pub struct ObserverPorts {
        input type ObserverInput {
            stats[Observe] => ModuleStats,
            loss[Observe] => Loss,
            metrics[Observe] => Metrics,
            ctrl[Control] => ControlSignal,
        }
        output type ObserverOutput {
            snapshot[Observe] => SystemSnapshot,
        }
    }
}
```

| Port | Direction | FlowKind | Type | Role |
|------|-----------|----------|------|------|
| `stats` | In | Observe | `ModuleStats` | Stats from all 5 Machines |
| `loss` | In | Observe | `Loss` | Latest training loss |
| `metrics` | In | Observe | `Metrics` | Latest evaluation metrics |
| `ctrl` | In | Control | `ControlSignal` | Track train_state transitions |
| `snapshot` | Out | Observe | `SystemSnapshot` | Aggregated system state |

**process() logic:**
```
match variant:
    stats   → update HashMap<module_name, ModuleStats>
    loss    → update latest_loss
    metrics → update latest_eval_loss + epoch
    ctrl    → update train_state

if elapsed > sample_interval_ms → yield snapshot (time-interval sampling)
else → Idle
```

---

## 3. Runtime Architecture

### 3.1 Channel Topology

```
main ──[DataLoaderInput]──▶ DataLoader ──[DataLoaderOutput]──▶ Router ──▶ BatcherInput ──▶ Batcher
                                                                       ──▶ ObserverInput ──▶ Observer

Batcher ──[BatcherOutput]──▶ Router ──▶ TrainerInput ──▶ Trainer
                                     ──▶ ObserverInput ──▶ Observer

Trainer ──[TrainerOutput]──▶ Router ──▶ EvaluatorInput ──▶ Evaluator
                                     ──▶ CheckpointerInput ──▶ Checkpointer
                                     ──▶ ObserverInput ──▶ Observer

Evaluator ──[EvaluatorOutput]──▶ Router ──▶ CheckpointerInput ──▶ Checkpointer
                                           ──▶ ObserverInput ──▶ Observer

Checkpointer ──[CheckpointerOutput]──▶ Router ──▶ ObserverInput ──▶ Observer

Observer ──[ObserverOutput]──▶ stdout
```

### 3.2 Cascade Shutdown

The shutdown sequence is a chain reaction:

```
1. Main drops loader_in_tx (after sending all ticks)
2. DataLoader exhausts samples → returns Done → drops loader_out_tx
3. Batcher's loader_out_rx returns None → returns Done → drops batcher_out_tx
4. Trainer's batcher_out_rx returns None → returns Done → drops trainer_out_tx
5a. Evaluator's trainer_out_rx returns None → returns Done → drops evaluator_out_tx
5b. Checkpointer's trainer_out_rx returns None → returns Done → drops checkpointer_out_tx
6. Observer waits for all upstream senders to drop:
   - loader_out_tx (data_loader stats)
   - batcher_out_tx (batcher stats)
   - trainer_out_tx (trainer stats + loss)
   - evaluator_out_tx (evaluator stats + metrics)
   - checkpointer_out_tx (checkpointer stats)
   - (ctrl channel already dropped by main)
   When all 5 upstream channels close → observer_out_rx returns None → Observer returns Done
```

### 3.3 Output Router Tasks

Each Machine's output channel is read by a dedicated router task that performs **port-variant–based fan-out**:

```rust
tokio::spawn(async move {
    while let Some(out) = rx.recv().await {
        match out {
            TrainerOutput::loss(l) => {
                if l.batch_id % 50 == 0 { println!("..."); }
                observer_tx.send(ObserverInput::loss(l)).await;
            }
            TrainerOutput::model_delta(d) => {
                evaluator_tx.send(EvaluatorInput::model_delta(d.clone())).await;
                checkpointer_tx.send(CheckpointerInput::model_delta(d)).await;
            }
            TrainerOutput::stats(st) => {
                observer_tx.send(ObserverInput::stats(st)).await;
            }
        }
    }
});
```

This demonstrates a key axiom pattern: **one output port variant routes to multiple downstream Machines** (model_delta → Evaluator + Checkpointer).

### 3.4 Thread Model

| Thread | Role | Machine |
|--------|------|---------|
| Tokio IO thread pool | Channel I/O + router tasks | (shared) |
| Rayon thread pool (auto) | Forward pass parallelism | Trainer (par_iter) |
| Tokio blocking pool | Machine::process() | All 6 Machines via spawn_blocking |

When `cpu_threads = 0` (default), Rayon uses all available cores.

---

## 4. Neural Network Module

A simple 3-layer MLP: `input → hidden1(ReLU) → hidden2(ReLU) → output(linear)`.

```
Layer 1: LinearLayer(2, 16)  → ReLU
Layer 2: LinearLayer(16, 8)  → ReLU
Layer 3: LinearLayer(8, 1)   → (linear)
Total parameters: 2*16 + 16 + 16*8 + 8 + 8*1 + 1 = 32 + 16 + 128 + 8 + 8 + 1 = 193
```

### 4.1 Forward Pass (Rayon Parallel)

```rust
let predictions: Vec<f64> = batch.features.par_iter()
    .map(|features| forward_only(&network, &Array1::from_vec(features.clone())))
    .collect();
```

The read-only forward pass is parallelized with Rayon. Only the backward pass is serial (requires `&mut Network`).

### 4.2 Synthetic Data

Target function: $y = \sin(x_1) + x_2^2 + \epsilon$, where:
- $x_1 \sim U(-5, 5)$
- $x_2 \sim U(-2, 2)$
- $\epsilon \sim U(-0.25, 0.25)$

8000 training samples + 2000 evaluation samples (default split: 80/20).

---

## 5. Design Decisions

### Why Channel (not BoundedBuf)?

All edges use `Channel` (MPSC) because:
- **Single consumer per port** — each output variant is consumed by exactly one or two downstream Machines
- **Async send** — non-blocking, no backpressure stalls in the pipeline
- **Cascade shutdown** — `recv()` returning `None` on sender drop is the natural shutdown signal

The `drop_when_full: false` setting on data edges ensures backpressure. The `drop_when_full: true` setting on observe edges to the Observer prevents observation traffic from blocking data flow (observe loss is acceptable; data loss is not).

### Why Output Routers (not Multiple Output Channels)?

Each Machine has a single `output: Sender<MachineOutput>` channel. A dedicated router task reads from this channel and dispatches by variant. This is simpler than managing multiple output channels per Machine and keeps the Machine implementation agnostic to the fan-out topology.

### Why Manual Channel Wiring (not DeploySpec Execution)?

The current axiom runtimes (LinearRuntime, TokioRuntime) run a single Machine with a vector of inputs. The training server needs:
- Multiple concurrent Machines with ongoing I/O
- Typed channel routing by port variant
- Cascade shutdown across 6 Machines

These are implemented as explicit Tokio tasks + mpsc channels. A future runtime could interpret `DeploySpec` + port enums directly to auto-generate the wiring.

---

## 6. File Reference

| File | Lines | Purpose |
|------|-------|---------|
| `src/main.rs` | ~500 | CLI, DeploySpec, channel wiring, spawn, routers |
| `src/types.rs` | ~130 | Shared data types: Sample, Batch, Loss, ModelDelta, Metrics, ControlSignal, ModuleStats, SystemSnapshot |
| `src/config.rs` | ~100 | TOML config loading with defaults |
| `src/machines/loader.rs` | ~145 | DataLoader: synthetic data generation + tick-driven output |
| `src/machines/batcher.rs` | ~88 | Batcher: sample → batch accumulation |
| `src/machines/trainer.rs` | ~180 | Trainer: forward/backward/SGD with Rayon |
| `src/machines/evaluator.rs` | ~195 | Evaluator: validation set evaluation |
| `src/machines/checkpointer.rs` | ~130 | Checkpointer: bincode model + JSONL metrics persistence |
| `src/machines/observer.rs` | ~170 | Observer: time-interval sampling + snapshot generation |
| `src/machines/controller.rs` | ~76 | Controller: CLI command dispatch (not used in start mode) |
| `src/nn/layer.rs` | ~85 | LinearLayer: weights, bias, forward, backward, zero_grad |
| `src/nn/network.rs` | ~85 | Network: 3-layer MLP orchestration |
| `src/nn/sgd.rs` | ~40 | SgdOptimizer: momentum SGD step |
