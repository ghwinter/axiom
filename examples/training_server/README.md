# Training Server — axiom Concurrent Neural Network Training

A production-style concurrent neural network training server built on **axiom**. Six Machines running on Tokio, connected by typed channels, demonstrating axiom's port-enum architecture, fan-out routing, observation sampling, and cascade shutdown.

## Quick Start

```bash
# Train (release mode, 10000 synthetic samples)
cargo run -p training_server --release -- --config examples/training_server/config.toml start

# Query training status (read persisted metrics)
cargo run -p training_server --release -- --config examples/training_server/config.toml status
```

### Sample Output

```
=== 并发训练服务器启动 ===
网络结构: 2 → 16 → 8 → 1
数据集大小: 10000
并发 Machine 数: 6
[topology] DeploySpec 校验通过: 6 machines, 12 links
[runtime] 所有 Machine 已 spawn，开始训练...

[trainer] batch=   1 epoch=0 loss=3.538278
[trainer] batch=  50 epoch=0 loss=2.107913
[02:29:05] state=Running batch=151 loss=2.493809 eval_loss=2.091943
[observe] state=Running batch=151 modules=5 loss=2.4938
...
=== 训练完成 ===
模型已保存到: output/model.bin
```

### Configuration

Edit `config.toml` to control:

| Section | Parameter | Default | Description |
|---------|-----------|---------|-------------|
| `training` | `dataset_size` | 10000 | Number of synthetic samples |
| `training` | `batch_size` | 32 | Samples per training batch |
| `training` | `learning_rate` | 0.01 | SGD learning rate |
| `training` | `epochs` | 10 | Training epochs |
| `network` | `hidden1_size` | 16 | First hidden layer width |
| `network` | `hidden2_size` | 8 | Second hidden layer width |
| `observe` | `sample_interval_ms` | 200 | Observer sampling interval |
| `persist` | `model_file` | `output/model.bin` | Model checkpoint path |

---

## Architecture

### Topology Diagram

```
  ┌──────────┐   sample    ┌──────────┐   batch    ┌──────────┐
  │DataLoader│ ──────────▶ │ Batcher  │ ──────────▶ │ Trainer  │
  │  input:  │             │  input:  │             │  input:  │
  │ ctrl     │             │ sample   │             │ batch    │
  │ tick     │             │  output: │             │ ctrl     │
  │ output:  │             │ batch    │             │  output: │
  │ sample   │             │ stats ──┐│             │ loss     │
  │ stats ──┐│             └─────────┘│             │ model_db │
  └─────────┘│                        │             │ stats ──┐│
             │                        │             └─────────┘│
             │                        │              │    │    │
             │              ┌─────────┘              │    │    │
             │              │          ┌──────────────┘    │    │
             ▼              ▼          ▼                   ▼    ▼
       ┌──────────────────────────────────────────────────────────┐
       │                        Observer                          │
       │  input: stats[] / loss / metrics / ctrl                   │
       │  output: snapshot (every sample_interval_ms)              │
       └──────────────────────────────────────────────────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │ snapshots.jsonl │
                    └──────────────┘

  model_delta ──▶ Evaluator ──▶ metrics ──▶ Checkpointer ──▶ metrics.jsonl
       │                                            │
       └────────────────────────────────────────────┘
                    model_delta ──▶ model.bin
```

### 6 Machines, 12 Typed Channels

| Machine | Input Ports | Output Ports | Role |
|---------|-------------|--------------|------|
| `DataLoader` | `ctrl[Control]`, `tick[Data]` | `sample[Data]`, `stats[Observe]` | Generate synthetic data samples |
| `Batcher` | `sample[Data]` | `batch[Data]`, `stats[Observe]` | Accumulate samples into fixed-size batches |
| `Trainer` | `batch[Data]`, `ctrl[Control]` | `loss[Data]`, `model_delta[Data]`, `stats[Observe]` | Forward+backward+ SGD update |
| `Evaluator` | `model_delta[Data]`, `ctrl[Control]` | `metrics[Data]`, `stats[Observe]` | Evaluate on held-out validation set |
| `Checkpointer` | `model_delta[Data]`, `metrics[Data]` | `stats[Observe]` | Persist model + metrics to disk |
| `Observer` | `stats[Observe]`, `loss[Observe]`, `metrics[Observe]`, `ctrl[Control]` | `snapshot[Observe]` | Low-frequency sampling + system snapshot |

### Data Flow

```
DataLoader  ──sample──▶ Batcher ──batch──▶ Trainer ──model_delta──▶ Evaluator
                                                    ├──model_delta──▶ Checkpointer
                                                    └──loss──────────▶ Observer (indirect)

Evaluator ──metrics──▶ Checkpointer
                      └──metrics──▶ Observer (indirect)

Every Machine ──stats──▶ Observer (via fan-out router)
Observer ──snapshot──▶ stdout + snapshots.jsonl
```

---

## Key axiom Concepts Demonstrated

### 1. Port Enum Architecture (declare_ports!)

```rust
declare_ports! {
    pub struct TrainerPorts {
        input type TrainerInput {
            batch[Data]    => Batch,
            ctrl[Control]  => ControlSignal,
        }
        output type TrainerOutput {
            loss[Data]        => Loss,
            model_delta[Data] => ModelDelta,
            stats[Observe]    => ModuleStats,
        }
    }
}
```

Each Machine declares its **interface set** (Gamma) as a port enum. The compiler enforces that variant names match port names, and `TypeId` is checked at link time.

### 2. Fan-Out via Output Routers

The Trainer produces three output variants in one tick:

```rust
ProcessOutput::YieldMulti(vec![
    TrainerOutput::loss(loss),
    TrainerOutput::model_delta(model_delta),
    TrainerOutput::stats(stats),
])
```

Output routers (async tasks) match on the variant and route to different downstream Machines:

```rust
while let Some(out) = trainer_out_rx.recv().await {
    match out {
        TrainerOutput::loss(l)          => observer_tx.send(ObserverInput::loss(l)).await,
        TrainerOutput::model_delta(d)   => { evaluator_tx.send(EvaluatorInput::model_delta(d.clone())).await;
                                             checkpointer_tx.send(CheckpointerInput::model_delta(d)).await; }
        TrainerOutput::stats(st)        => observer_tx.send(ObserverInput::stats(st)).await,
    }
}
```

### 3. Observe Isolation

Observe outputs (`stats`, `loss`, `metrics`) flow to the Observer only — they never participate in data-flow computation. This enforces Theorem 2.2 (observability isolation): Observe data is not consumed by any Machine's `process()`.

### 4. Control Plane

The `ctrl[Control]` ports on DataLoader, Trainer, Evaluator, and Observer accept `ControlSignal` (Start/Pause/Resume/Stop). Control is just Data with a different `FlowKind` label — same channel mechanism, same type safety.

### 5. Cascade Shutdown

When DataLoader exhausts its samples, it returns `ProcessOutput::Done`. This drops its output sender → the channel to Batcher closes → Batcher's `recv()` returns `None` → Batcher returns `Done` → cascade continues. No explicit shutdown coordination needed.

### 6. Initial Value Injection

Config is injected via `MachineContext::set_initial_value()` before spawning:

```rust
let mut ctx = MachineContext::new("data_loader");
ctx.set_initial_value(config.clone());
```

Each Machine retrieves it in `init()`:

```rust
fn init(ctx: &MachineContext) -> Result<DataLoaderState, InitError> {
    let config = ctx.initial_value::<Config>().expect("DataLoader needs Config");
    // ...
}
```

### 7. Time-Interval Observation Sampling

The Observer only emits a snapshot every `sample_interval_ms` (default 200ms), preventing stdout from being overwhelmed by per-batch updates:

```rust
if elapsed_ms >= state.sample_interval_ms {
    ProcessOutput::Yield(ObserverOutput::snapshot(snapshot))
} else {
    ProcessOutput::Idle
}
```

---

## Project Structure

```
examples/training_server/
├── Cargo.toml              # Dependencies: axiom, axiom-tokio, tokio, ndarray, rayon, clap
├── config.toml             # Training/network/observe/persist/runtime configuration
├── src/
│   ├── main.rs             # CLI (start/status/interactive), DeploySpec, output routers, spawn
│   ├── types.rs            # Shared types: Sample, Batch, Loss, ModelDelta, Metrics, ControlSignal, etc.
│   ├── config.rs           # Config loading from TOML
│   ├── machines/
│   │   ├── mod.rs          # Re-exports
│   │   ├── loader.rs       # DataLoader Machine — synthetic data generation
│   │   ├── batcher.rs      # Batcher Machine — sample → batch accumulation
│   │   ├── trainer.rs      # Trainer Machine — forward/backward/SGD with Rayon parallelism
│   │   ├── evaluator.rs    # Evaluator Machine — validation set evaluation
│   │   ├── checkpointer.rs # Checkpointer Machine — model/metrics persistence
│   │   ├── observer.rs     # Observer Machine — low-frequency sampling + snapshots
│   │   └── controller.rs   # Controller Machine — CLI command dispatch (future use)
│   └── nn/
│       ├── mod.rs          # Re-exports
│       ├── layer.rs        # LinearLayer (weights + bias + forward/backward/zero_grad)
│       ├── network.rs      # Network (3-layer MLP: forward/backward/zero_grad/weights_flatten)
│       └── sgd.rs          # SgdOptimizer (momentum SGD)
└── docs/
    └── architecture.md     # Detailed architecture documentation
```

## Running

```bash
# Development (unoptimized, faster compile)
cargo run -p training_server -- --config examples/training_server/config.toml start

# Release (optimized, ~10x faster training)
cargo run -p training_server --release -- --config examples/training_server/config.toml start

# Check training metrics after completion
cargo run -p training_server --release -- --config examples/training_server/config.toml status

# Interactive mode
cargo run -p training_server --release -- --config examples/training_server/config.toml interactive
```
