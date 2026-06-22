# Training Server — Diagrams

> Mermaid architecture, topology, and lifecycle diagrams for the concurrent neural network training server.
> Render with any Mermaid-compatible viewer.

---

## 1. System Topology Overview

```mermaid
graph TB
  subgraph DataPlane["Data Flow"]
    DL["DataLoader
    input: ctrl, tick
    output: sample, stats"]
    BT["Batcher
    input: sample
    output: batch, stats"]
    TR["Trainer
    input: batch, ctrl
    output: loss, model_delta, stats"]
    EV["Evaluator
    input: model_delta, ctrl
    output: metrics, stats"]
    CK["Checkpointer
    input: model_delta, metrics
    output: stats"]
  end

  subgraph ObservePlane["Observe Flow"]
    OB["Observer
    input: stats[], loss, metrics, ctrl
    output: snapshot"]
  end

  subgraph Persist["Persistence"]
    MB["model.bin
    (bincode)"]
    MJ["metrics.jsonl
    (JSONL)"]
    SJ["snapshots.jsonl
    (JSONL)"]
  end

  DL -->|"sample[Data]"| BT
  BT -->|"batch[Data]"| TR
  TR -->|"model_delta[Data]"| EV
  TR -->|"model_delta[Data]"| CK
  EV -->|"metrics[Data]"| CK
  TR -.->|"loss[Observe]"| OB
  EV -.->|"metrics[Observe]"| OB
  DL -.->|"stats[Observe]"| OB
  BT -.->|"stats[Observe]"| OB
  TR -.->|"stats[Observe]"| OB
  EV -.->|"stats[Observe]"| OB
  CK -.->|"stats[Observe]"| OB

  CK -->|"model.bin"| MB
  CK -->|"JSONL append"| MJ
  OB -->|"JSONL append"| SJ
```

---

## 2. Port Interface Sets (per Machine)

```mermaid
graph LR
  subgraph DL_Ports["DataLoader Ports"]
    direction TB
    DLI["Input enum = DataLoaderInput
    ctrl [Control] => ControlSignal
    tick [Data] => u64"]
    DLO["Output enum = DataLoaderOutput
    sample [Data] => Sample
    stats [Observe] => ModuleStats"]
  end

  subgraph BT_Ports["Batcher Ports"]
    direction TB
    BTI["Input enum = BatcherInput
    sample [Data] => Sample"]
    BTO["Output enum = BatcherOutput
    batch [Data] => Batch
    stats [Observe] => ModuleStats"]
  end

  subgraph TR_Ports["Trainer Ports"]
    direction TB
    TRI["Input enum = TrainerInput
    batch [Data] => Batch
    ctrl [Control] => ControlSignal"]
    TRO["Output enum = TrainerOutput
    loss [Data] => Loss
    model_delta [Data] => ModelDelta
    stats [Observe] => ModuleStats"]
  end

  subgraph EV_Ports["Evaluator Ports"]
    direction TB
    EVI["Input enum = EvaluatorInput
    model_delta [Data] => ModelDelta
    ctrl [Control] => ControlSignal"]
    EVO["Output enum = EvaluatorOutput
    metrics [Data] => Metrics
    stats [Observe] => ModuleStats"]
  end

  subgraph CK_Ports["Checkpointer Ports"]
    direction TB
    CKI["Input enum = CheckpointerInput
    model_delta [Data] => ModelDelta
    metrics [Data] => Metrics"]
    CKO["Output enum = CheckpointerOutput
    stats [Observe] => ModuleStats"]
  end

  subgraph OB_Ports["Observer Ports"]
    direction TB
    OBI["Input enum = ObserverInput
    stats [Observe] => ModuleStats
    loss [Observe] => Loss
    metrics [Observe] => Metrics
    ctrl [Control] => ControlSignal"]
    OBO["Output enum = ObserverOutput
    snapshot [Observe] => SystemSnapshot"]
  end
```

---

## 3. Data Processing Pipeline

```mermaid
sequenceDiagram
  participant Main
  participant DL as DataLoader
  participant BT as Batcher
  participant TR as Trainer
  participant EV as Evaluator
  participant CK as Checkpointer
  participant OB as Observer

  Main->>DL: DataLoaderInput::ctrl(Start)
  loop For each of 10000 samples
    Main->>DL: DataLoaderInput::tick(seq)
    DL->>DL: generate synthetic sample
    DL->>BT: DataLoaderOutput::sample(sample)
    DL->>OB: stats[Observe]
  end
  Main->>DL: drop sender (cascade trigger)
  DL->>DL: cursor >= len -> return Done

  loop accumulate batch_size samples
    BT->>BT: buffer.push(sample)
    alt buffer.len() >= batch_size
      BT->>TR: BatcherOutput::batch(batch)
      BT->>OB: stats[Observe]
    else
      BT->>BT: idle (wait for more)
    end
  end
  BT->>BT: recv None -> return Done

  loop For each batch
    TR->>TR: forward pass (Rayon parallel)
    TR->>TR: compute MSE loss
    TR->>TR: backward pass + SGD update
    TR->>EV: model_delta
    TR->>CK: model_delta
    TR->>OB: loss[Observe]
    TR->>OB: stats[Observe]
    alt batch_id % 50 == 0
      TR-->>Main: stdout loss
    end
  end
  TR->>TR: recv None -> return Done

  EV->>EV: evaluate on validation set
  EV->>CK: metrics
  EV->>OB: metrics[Observe]
  EV->>OB: stats[Observe]
  EV->>EV: recv None -> return Done

  CK->>CK: serialize weights (bincode)
  CK->>CK: append metrics (JSONL)
  CK->>OB: stats[Observe]
  CK->>CK: recv None -> return Done

  OB->>OB: every 200ms: snapshot
  OB->>OB: write snapshots.jsonl
  OB-->>Main: stdout summary
  OB->>OB: all channels closed -> return Done
```

---

## 4. Cascade Shutdown Sequence

```mermaid
graph TB
  subgraph Step1["1. Main drops loader_in_tx"]
    A1["DataLoader recv() returns None"]
    A2["DataLoader returns Done"]
    A3["loader_out_tx dropped"]
  end

  subgraph Step2["2. Batcher channels close"]
    B1["Batcher recv() returns None"]
    B2["Batcher returns Done"]
    B3["batcher_out_tx dropped"]
  end

  subgraph Step3["3. Trainer channels close"]
    C1["Trainer recv() returns None"]
    C2["Trainer returns Done"]
    C3["trainer_out_tx dropped"]
  end

  subgraph Step4a["4a. Evaluator closes"]
    D1["Evaluator recv() returns None"]
    D2["Evaluator returns Done"]
    D3["evaluator_out_tx dropped"]
  end

  subgraph Step4b["4b. Checkpointer closes"]
    E1["Checkpointer recv() returns None"]
    E2["Checkpointer returns Done"]
    E3["checkpointer_out_tx dropped"]
  end

  subgraph Step5["5. Observer closes"]
    F1["All 5 upstream senders dropped"]
    F2["Observer recv() returns None"]
    F3["Observer returns Done"]
  end

  subgraph Step6["6. All tasks joined"]
    G1["main().await completes"]
  end

  A1 --> A2 --> A3 --> B1
  B1 --> B2 --> B3 --> C1
  C1 --> C2 --> C3
  C3 --> D1
  C3 --> E1
  D1 --> D2 --> D3 --> F1
  E1 --> E2 --> E3 --> F1
  F1 --> F2 --> F3 --> G1
```

---

## 5. Output Router Fan-out

```mermaid
graph TB
  subgraph TR_Out["Trainer Output Channel"]
    TR["Trainer::process()
    returns YieldMulti(vec![
      loss,
      model_delta,
      stats
    ])"]
  end

  subgraph Router["Router Task (tokio::spawn)"]
    direction TB
    R["while let Some(out) = rx.recv() {
    match out"]
    RL["TrainerOutput::loss(l)"]
    RD["TrainerOutput::model_delta(d)"]
    RS["TrainerOutput::stats(st)"]
  end

  subgraph Downstream["Downstream Targets"]
    EV_IN["EvaluatorInput::model_delta"]
    CK_IN1["CheckpointerInput::model_delta"]
    CK_IN2["CheckpointerInput::metrics"]
    OB_IN1["ObserverInput::loss"]
    OB_IN2["ObserverInput::stats"]
    OB_IN3["ObserverInput::metrics"]
  end

  subgraph EV_Out["Evaluator Output Channel"]
    EV["Evaluator::process()
    returns YieldMulti(vec![
      metrics,
      stats
    ])"]
  end

  subgraph Router2["Router Task (tokio::spawn)"]
    R2["match out"]
    R2M["EvaluatorOutput::metrics(m)"]
    R2S["EvaluatorOutput::stats(st)"]
  end

  TR --> R
  R --> RL
  R --> RD
  R --> RS
  RL -->|"every 50th -> stdout"| OB_IN1
  RD --> EV_IN
  RD --> CK_IN1
  RS --> OB_IN2

  EV --> R2
  R2 --> R2M
  R2 --> R2S
  R2M --> CK_IN2
  R2M --> OB_IN3
  R2S --> OB_IN2
```

---

## 6. Thread Model

```mermaid
graph TB
  subgraph MainThread["Main thread (tokio::main)"]
    M1["CLI parsing"]
    M2["DeploySpec::validate()"]
    M3["Channel allocation"]
    M4["Spawn routers + Machines"]
    M5["Send Start + ticks"]
    M6["await handles (join)"]
  end

  subgraph TokioPool["Tokio IO thread pool"]
    direction TB
    R1["Router: DataLoader output"]
    R2["Router: Batcher output"]
    R3["Router: Trainer output"]
    R4["Router: Evaluator output"]
    R5["Router: Checkpointer output"]
    R6["Router: Observer output"]
  end

  subgraph BlockingPool["Tokio blocking pool (spawn_blocking)"]
    direction TB
    DL_Proc["DataLoader::process()"]
    BT_Proc["Batcher::process()"]
    TR_Proc["Trainer::process()"]
    EV_Proc["Evaluator::process()"]
    CK_Proc["Checkpointer::process()"]
    OB_Proc["Observer::process()"]
  end

  subgraph RayonPool["Rayon thread pool"]
    FW["Forward pass (par_iter)
    read-only, parallel"]
  end

  MainThread -->|"spawn"| TokioPool
  MainThread -->|"spawn_blocking"| BlockingPool
  TR_Proc -->|"forward_only()"| RayonPool
  TokioPool <-->|"mpsc channels"| BlockingPool
```

---

## 7. Neural Network Data Flow

```mermaid
graph LR
  subgraph Input["Input: batch of 32 samples"]
    I1["features: Vec&lt;Vec&lt;f64&gt;&gt;
    each sample: [x1, x2]
    label: y = sin(x1) + x2^2 + noise"]
  end

  subgraph Forward["Forward pass (Rayon parallel)"]
    L1["Layer 1: Linear(2, 16) + ReLU"]
    L2["Layer 2: Linear(16, 8) + ReLU"]
    L3["Layer 3: Linear(8, 1)  (linear)"]
  end

  subgraph Loss["Loss computation"]
    MSE["MSE = mean(pred - label)^2"]
  end

  subgraph Backward["Backward pass (serial)"]
    B1["dL/dw3 = h2^T * dL/dy"]
    B2["dL/dw2 = h1^T * dL/dh2"]
    B3["dL/dw1 = x^T * dL/dh1"]
  end

  subgraph Update["SGD update"]
    S1["v = momentum * v + lr * grad"]
    S2["w = w - v"]
  end

  subgraph Output["Outputs"]
    O1["Loss { batch_id, loss, epoch }"]
    O2["ModelDelta { epoch, weights }"]
  end

  I1 --> L1
  L1 --> L2
  L2 --> L3
  L3 --> MSE
  MSE --> Backward
  Backward --> Update
  Update --> O1
  Update --> O2

  note right of Forward: "32 samples in parallel
  using rayon::par_iter()
  read-only access to weights"
```

---

## 8. State Machine Transitions

```mermaid
stateDiagram-v2
  state "DataLoader" as DL {
    [*] --> Idle: init()
    Idle --> Running: ctrl(Start | Resume)
    Running --> Idle: ctrl(Pause | Stop)
    Running --> [*]: cursor >= n (Done)
  }

  state "Batcher" as BT {
    [*] --> Accumulating: init()
    Accumulating --> Accumulating: sample received
    Accumulating --> Emitting: buffer >= batch_size
    Emitting --> Accumulating: continue
    Accumulating --> [*]: recv() = None (Done)
  }

  state "Trainer" as TR {
    [*] --> Waiting: init()
    Waiting --> Training: ctrl(Start)
    Training --> Training: batch received + process
    Training --> Waiting: ctrl(Pause)
    Training --> [*]: recv() = None (Done)
  }

  state "Observer" as OB {
    [*] --> Listening: init()
    Listening --> Sampling: elapsed >= interval
    Sampling --> Listening: snapshot emitted
    Listening --> [*]: all senders dropped (Done)
  }

  state ProcessOutput {
    state "Yield" as Y: single output on one port
    state "YieldMulti" as YM: multiple outputs, fan-out
    state "Idle" as I: no output this tick
    state "Done" as D: machine finished, cascade shutdown
  }
```

---

## 9. DeploySpec Topology (Graph View)

```mermaid
graph LR
  subgraph Spec["DeploySpec (6 machines, 12 links)"]
    direction TB
    V1["data_loader: DataLoader"]
    V2["batcher: Batcher"]
    V3["trainer: Trainer"]
    V4["evaluator: Evaluator"]
    V5["checkpointer: Checkpointer"]
    V6["observer: Observer"]
  end

  V1 -->|"sample"| V2
  V1 -->|"stats"| V6
  V2 -->|"batch"| V3
  V2 -->|"stats"| V6
  V3 -->|"loss"| V6
  V3 -->|"model_delta"| V4
  V3 -->|"model_delta"| V5
  V3 -->|"stats"| V6
  V4 -->|"metrics"| V5
  V4 -->|"metrics"| V6
  V4 -->|"stats"| V6
  V5 -->|"stats"| V6
```

---



---

## 10. One Complete Training Tick

``mermaid
gantt
  title One complete training run (10000 samples, batch_size=32)
  dateFormat  HH:mm:ss
  axisFormat  %M:%S

  section DataLoader
  init + generate 10000 samples  :a1, 00:00:00, 00:00:02
  send tick * 10000              :a2, 00:00:02, 00:00:05
  Done                            :a3, 00:00:05, 00:00:01

  section Batcher
  recv samples -> emit batches   :b1, 00:00:05, 00:00:20

  section Trainer
  forward (Rayon)                 :c1, 00:00:05, 00:00:10
  backward + SGD                  :c2, 00:00:10, 00:00:15
  emit outputs                    :c3, 00:00:15, 00:00:01

  section Evaluator
  recv model_delta + eval         :d1, 00:00:15, 00:00:05
  emit metrics                    :d2, 00:00:20, 00:00:01

  section Observer
  snapshot every 200ms            :e1, 00:00:05, 00:00:25
``
