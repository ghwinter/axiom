# axiom Architecture Diagrams

> Mermaid diagrams. Render with any Mermaid-compatible viewer.

---

## 1. Core Architecture

### 1.1 Three-Layer Structure

```mermaid
graph TB
  subgraph Layer2["Layer 2: Machine (Computation)"]
    direction TB
    T["trait Machine {
      type State: Send + 'static
      type Input: HasPortInfo   (port enum)
      type Output: HasPortInfo  (port enum)
      type Ports: PortSet       (auto-derived port_schema)
      fn name()
      fn port_schema()  -- auto
      fn config_schema()
      fn init(ctx) -> State
      fn process(&mut State, ctx, Input) -> ProcessOutput<Output>
      fn cleanup(State, ctx) -> Result
      fn physical_spec() -> MachinePhysicalSpec
      fn deterministic() -> bool
      fn checkpoint / restore  (optional)
    }"]
  end

  subgraph Layer1["Layer 1: Ports (Communication)"]
    P1["PortSchema = { PortDecl[] }"]
    P2["PortDecl { name, dir, flow, type_id }"]
    P3["ConfigSchema = { ConfigDecl[] }"]
    P4["MachineContext { lifecycle, observe, signal, time, snapshot }"]
  end

  subgraph Layer0["Layer 0: Entity (Existence)"]
    E["trait Entity {
      type State: Send + 'static
      fn name()
      fn physical_spec()
      fn checkpoint / restore
    }
    -- minimal persistent existence
       no ports, no process"]
  end

  Layer2 --> Layer1
  Layer1 --> Layer0
```

### 1.2 Port Three-Axis Design

```mermaid
graph LR
  subgraph Port["PortDecl 3 axes"]
    D["Direction: In | Out"]
    F["FlowKind: Data | Control | Observe"]
    T["Type: any Rust type (TypeId)"]
  end

  subgraph Builders["Convenience constructors"]
    B1["input::&lt;T&gt;('name') = In + Data"]
    B2["output::&lt;T&gt;('name') = Out + Data"]
    B3["ctrl_in::&lt;T&gt;('name') = In + Control"]
    B4["ctrl_out::&lt;T&gt;('name') = Out + Control"]
    B5["observe::&lt;T&gt;('name') = Out + Observe"]
  end

  subgraph Check["LinkCompat checks"]
    C1["direction: out -> in"]
    C2["type_id: must match"]
    C3["flow: must match"]
    C4["version drift: |a-b| <= 1"]
  end

  Port --> Builders
  Port --> Check
```

### 1.3 Two Computation Primitives

```mermaid
graph LR
  subgraph Func["Func (pure function)"]
    F1["Memory: Stack frame"]
    F2["State: None"]
    F3["Life: single call, unobservable"]
    F4["Connect: inline call only"]
    F5["Parallel: fully safe"]
    F6["Cost: zero (direct fn call)"]
    F7["Deterministic: guaranteed"]
    F8["Bridge: FuncMachine&lt;F&gt; -> Machine"]
  end

  subgraph Machine["Machine (state machine)"]
    M1["Memory: Heap"]
    M2["State: type State (persistent)"]
    M3["Life: init -> process* -> cleanup"]
    M4["Connect: ports (6 LinkKinds)"]
    M5["Parallel: &mut State per-instance serial"]
    M6["Cost: depends on LinkKind + runtime"]
    M7["Deterministic: opt-in declaration"]
    M8["Observe: FlowKind::Observe ports"]
  end

  Func -->|"FuncMachine embeds"| Machine
```

### 1.4 Six Link Strategies

```mermaid
graph TB
  subgraph Links["LinkKind"]
    I["Inline
    fn call, zero alloc
    caller blocks
    same-thread only"]
    B["BoundedBuf
    bounded ring buffer
    3 write / 2 read policies
    cross-thread, backpressure"]
    C["Channel
    MPSC channel
    multi-producer single-consumer
    async messaging"]
    L["Latest
    single overwrite slot
    always newest value
    status push"]
    CF["CasFreeRing
    lock-free SPSC
    fixed address
    embedded / ISR-to-main"]
    S["SharedState
    Arc&lt;RwLock&lt;T&gt;&gt;
    n:m arbitrary
    config distribution"]
  end

  subgraph Constraints["Edge constraints"]
    C1["Inline: must be a DAG (no cycles)"]
    C2["Channel: indeg(dst) = 1"]
    C3["CasFreeRing: indeg <=1, outdeg <=1"]
    C4["Others: no degree constraints"]
  end

  Links --> Constraints
```

### 1.5 Deployment System

```mermaid
graph TB
  subgraph Spec["DeploySpec (pure data, no execution)"]
    direction LR
    MI["MachineInstance[]
    {name, type, physical, config}"]
    FB["FuncBinding[]
    {name, type}"]
    LS["LinkSpec[]
    {out, into, kind}"]
    DS["DeploySettings
    {cpu_threads, io_threads}"]
  end

  Spec --> Validate["validate()
  machine name existence
  port type matching
  (TODO: cycle detection)"]

  Validate --> Runtimes["Runtime adapters"]

  subgraph RuntimesBox["Runtime adapters"]
    R1["axiom_linear
    single-thread for-loop
    zero-alloc Inline links"]
    R2["axiom_tokio
    Tokio multi-thread
    spawn_blocking"]
    R3["axiom_replay (planned)
    deterministic single-thread
    backtest / simulation"]
    R4["axiom_embassy (planned)
    no_std embedded
    Embassy async"]
  end

  subgraph Exec["ExecutionHint"]
    E1["Async -- cooperative multitask"]
    E2["CpuBound -- dedicated OS thread"]
    E3["CpuBoundN(n) -- N threads"]
    E4["ThreadPool -- private bounded pool"]
    E5["Subprocess -- isolated process (IPC)"]
  end

  RuntimesBox --> Exec
```

### 1.6 Algebraic Structure

```mermaid
graph TB
  subgraph IOObj["IO-Object = (S, I, O, delta, rho)"]
    S["S = State space (Heap)"]
    I["I = interface set Gamma_in (port enum)"]
    O["O = interface set Gamma_out (port enum)"]
    delta["delta: S x I -> S x O = process()"]
    rho["rho: S -> S = cleanup()"]
  end

  subgraph Category["Category theory"]
    CAT1["Identity: builtin::Identity&lt;I&gt;"]
    CAT2["Composition ⨟: FuncScratchPipeline"]
    CAT3["Initial object: EntityRoot"]
    CAT4["Embedding functor: FuncMachine&lt;F&gt;"]
  end

  subgraph Theorems["Core theorems"]
    TH1["T1.1: Pure fn isolation -> parallel safe"]
    TH2["T2.1: Type safety -> TypeId checked at link"]
    TH3["T2.2: Observe isolation -> Obs not in delta input"]
    TH4["T5.2: Category laws -> composition closed + associative + unit"]
    TH5["T6.1: Deployment invariance -> delta independent of execution strategy"]
    TH6["T7.2: Observability completeness -> observe reachable iff link exists"]
  end
```

---

## 2. Complex Application: Smart Building Energy Management

### 2.1 System Topology

```mermaid
graph TB
  subgraph Sensors["Perception Layer (Sensors)"]
    ST["TempSensor (x8)
    per_zone_temperature
    Data: f64"]
    SO["OccSensor (x8)
    per_zone_occupancy
    Data: Occupancy"]
    SP["PowerSensor (x4)
    per_circuit_power
    Data: f64"]
    SW["WeatherSource
    weather_feed
    Data: WeatherData"]
  end

  subgraph Controllers["Control Layer (Controllers)"]
    HVAC["HVAC Controller
    input: temp/setpoint/mode
    output: valve/fan
    ctrl_in: mode switch/target
    observe: runtime status"]
    LIGHT["Lighting Controller
    input: occupancy/light
    output: dim command
    ctrl_in: scene switch
    observe: energy stats"]
    SHADE["Shade Controller
    input: light/weather
    output: blind angle
    ctrl_in: mode switch
    observe: position"]
  end

  subgraph Opt["Optimization Layer"]
    EO["Energy Optimizer
    input: aggregated metrics/price
    output: setpoints to controllers
    ctrl_in: strategy switch
    observe: optimization gain"]
  end

  subgraph Safety["Safety Layer"]
    FM["Fire Safety Interface
    input: smoke/alarm
    ctrl_out: emergency stop to all
    observe: system status"]
  end

  subgraph Obs["Observation Layer"]
    COLL["Data Collector
    input: observe streams from all
    output: none (persist to DB)"]
    DASH["Dashboard
    input: aggregated metrics
    output: visual data"]
    ALERT["Alert Engine
    input: anomaly metrics
    ctrl_out: alert notifications"]
  end

  subgraph Store["Persistence Layer"]
    TSDB["Time-Series DB
    input: time-series data
    observe: query results"]
  end

  ST -->|"zone_temp"| HVAC
  SO -->|"zone_occ"| LIGHT
  SP -->|"power"| EO
  SW -->|"weather"| SHADE

  HVAC -->|"hvac_runtime"| COLL
  LIGHT -->|"light_stats"| COLL
  SHADE -->|"shade_status"| COLL
  EO -->|"optimizer_stats"| COLL
  FM -->|"safety_status"| COLL

  HVAC -->|"zone_temp_avg"| EO
  LIGHT -->|"light_power"| EO

  EO -->|"temp_setpoint"| HVAC
  EO -->|"light_schedule"| LIGHT
  EO -->|"shade_strategy"| SHADE

  FM -->|"emergency_stop"| HVAC
  FM -->|"emergency_stop"| LIGHT
  FM -->|"emergency_stop"| SHADE

  COLL -->|"stored_data"| TSDB
  TSDB -->|"query_results"| DASH
  COLL -->|"metrics"| ALERT
  ALERT -->|"alert_action"| EO
```

### 2.2 Port Declaration Examples

```mermaid
graph LR
  subgraph HVAC_Ports["HVAC Controller Ports"]
    direction TB
    HVI["Input enum = HvacInput
    zone_temp [Data] => f64
    temp_setpoint [Data] => f64
    mode_switch [Control] => ModeCmd
    emergency_stop [Control] => StopSignal"]

    HVO["Output enum = HvacOutput
    valve_position [Data] => f64
    fan_speed [Data] => FanSpeed
    zone_temp_avg [Data] => AggregatedTemp
    runtime_status [Observe] => StatusReport"]
  end

  subgraph EO_Ports["Energy Optimizer Ports"]
    direction TB
    EO_I["Input enum = OptimizerInput
    zone_temp_avg [Data] => AggregatedTemp
    light_power [Data] => f64
    electricity_price [Data] => f64
    strategy_switch [Control] => StrategyCmd
    alert_action [Control] => AlertAction"]

    EO_O["Output enum = OptimizerOutput
    temp_setpoint [Data] => f64
    light_schedule [Data] => LightSchedule
    shade_strategy [Data] => ShadeStrategy
    optimization_gain [Observe] => OptimizationReport"]
  end

  subgraph FS_Ports["Fire Safety Ports"]
    direction TB
    FS_I["Input enum = SafetyInput
    smoke_alarm [Data] => AlarmSignal
    manual_override [Control] => OverrideCmd"]

    FS_O["Output enum = SafetyOutput
    emergency_stop [Control] => StopSignal
    safety_status [Observe] => SystemHealth"]
  end
```

### 2.3 Deployment Topology (Multi-threaded)

```mermaid
graph TB
  subgraph T1["Thread 1: IO (sensor polling)"]
    TS1["TempSensor_1"]
    TS2["TempSensor_2"]
    OS1["OccSensor_1"]
  end

  subgraph T2["Thread 2: IO (data collection)"]
    COLL
    TSDB
  end

  subgraph T3["Thread 3: control computation"]
    HVAC
    LIGHT
    SHADE
    EO["EnergyOpt"]
    FM["FireSafety"]
  end

  subgraph T4["Thread 4: alerting"]
    ALERT
  end

  TS1 -->|"BoundedBuf"| HVAC
  TS2 -->|"BoundedBuf"| HVAC
  OS1 -->|"BoundedBuf"| LIGHT

  HVAC -->|"BoundedBuf"| EO
  EO -->|"BoundedBuf"| HVAC
  LIGHT -->|"BoundedBuf"| EO
  SHADE -->|"BoundedBuf"| EO
  FM -->|"BoundedBuf"| HVAC
  FM -->|"BoundedBuf"| LIGHT
  FM -->|"BoundedBuf"| SHADE

  HVAC -->|"Latest"| COLL
  LIGHT -->|"Latest"| COLL
  SHADE -->|"Latest"| COLL
  EO -->|"Latest"| COLL
  FM -->|"Latest"| COLL

  COLL -->|"Inline"| TSDB
  COLL -->|"Channel"| ALERT
  ALERT -->|"BoundedBuf"| EO
```

### 2.4 Deployment Mapping (Graph Homomorphism)

```mermaid
graph TB
  subgraph Abstract["Abstract Topology Sigma_abstract"]
    direction LR
    S["Sensors (x21)"]
    C["Controllers (x3)"]
    O["Optimizer"]
    F["FireSafety"]
    M["Monitoring"]

    S -->|"Data"| C
    C -->|"Data"| O
    O -->|"Control"| C
    F -->|"Control"| C
    C -->|"Observe"| M
    O -->|"Observe"| M
    F -->|"Observe"| M
  end

  Abstract --> Delta["Deployment mapping Delta"]

  subgraph Physical["Physical Topology Sigma_physical"]
    direction LR
    subgraph T1_P["Thread 1 (IO)"]
      S1_P["TempSensor_1"]
      S2_P["TempSensor_2"]
    end
    subgraph T3_P["Thread 3 (compute)"]
      HVAC_P["HVAC"]
      LIGHT_P["Light"]
      EO_P["EnergyOpt"]
      FM_P["FireSafety"]
    end
    subgraph T2_P["Thread 2 (IO)"]
      COLL_P["Collector"]
      TSDB_P["TSDB"]
    end
    subgraph T5_P["Thread 5 (alert)"]
      ALERT_P["Alert"]
    end

    S1_P -->|"BoundedBuf"| HVAC_P
    S2_P -->|"BoundedBuf"| HVAC_P
    HVAC_P -->|"BoundedBuf"| EO_P
    EO_P -->|"BoundedBuf"| HVAC_P
    FM_P -->|"BoundedBuf"| HVAC_P
    HVAC_P -->|"Latest"| COLL_P
    COLL_P -->|"Inline"| TSDB_P
    COLL_P -->|"Channel"| ALERT_P
    ALERT_P -->|"BoundedBuf"| EO_P
  end

  Delta --> Note["Delta preserves:
    vertex -> thread assignment
    edge -> LinkKind substitution
    Inline -> BoundedBuf (cross-thread)
    Inline -> Inline (same-thread)
    connectivity preserved"]
```

### 2.5 Static Graph Analysis

```mermaid
graph TB
  subgraph Invariants["Invariant checks"]
    I1["Type compat: all edges type_id match"]
    I2["FlowKind: all edges flow match"]
    I3["No Inline cycles: needs topological sort (TODO)"]
    I4["Degree: Channel indeg=1, CasFreeRing SPSC (TODO)"]
  end

  subgraph Feedback["Feedback topology"]
    F1["HVAC -> EO -> HVAC: BoundedBuf edges"]
    F2["legal feedback loop (1-tick delay)"]
    F3["No Inline cycles -> no algebraic loops"]
    F4["Moore-type machines safe in feedback"]
  end

  subgraph SPOF["Single point of failure"]
    S1["Energy Optimizer is SPOF"]
    S2["all controllers depend on its setpoints"]
    S3["mitigation: CpuBoundN(2) redundant instance"]
    S4["TempSensors: 8 parallel, single failure safe"]
  end

  subgraph ObsComplete["Observability completeness"]
    O1["All modules have observe paths to Collector"]
    O2["Collector data reaches Dashboard + Alert"]
    O3["Theorem 7.2 satisfied"]
  end
```

### 2.6 Runtime Lifecycle (5 timesteps)

```mermaid
---
displayMode: compact
---
gantt
  title Building Management System - 5 timesteps
  dateFormat  HH:mm
  axisFormat  %H:%M

  section t=0: Init
  DeploySpec::validate()           :00:00, 1min
  Machine::init() all modules      :00:01, 1min
  Lifecycle: Init -> Running       :00:02, 1min

  section t=1: Normal
  TempSensor 24.5C -> HVAC         :00:03, 1min
  HVAC -> EnergyOpt 24.3C          :00:04, 1min
  EnergyOpt -> HVAC setpoint 22C   :00:05, 1min
  HVAC -> Collector status OK      :00:06, 1min
  Collector -> TSDB write          :00:07, 1min

  section t=2: Optimization
  Electricity price signal         :00:08, 1min
  EnergyOpt new strategy           :00:09, 1min
  Setpoint -> 23.5C (eco)         :00:10, 1min
  Light schedule -> dim(70%)       :00:11, 1min
  Shade strategy -> max solar      :00:12, 1min

  section t=3: Safety Event
  FireSafety smoke alarm           :00:13, 1min
  emergency_stop to all            :00:14, 1min
  HVAC Lifecycle: Running->Stopping:00:15, 1min
  All controllers override         :00:16, 1min

  section t=4: Cleanup
  HVAC::cleanup() close valves     :00:17, 1min
  LIGHT::cleanup() emergency mode  :00:18, 1min
  SHADE::cleanup() open blinds     :00:19, 1min
  Lifecycle: Stopping -> Stopped   :00:20, 1min
  Collector final report to disk   :00:21, 1min
```

---

## Design Summary

| Feature | Demonstrated in this scenario |
|---------|-------------------------------|
| Heterogeneous multi-port | HVAC: 4 input types (temp/setpoint/mode/estop) + 4 output types (valve/fan/aggregate/status) |
| Multi-source fan-in | 8 TempSensors -> HVAC -> EnergyOpt (hierarchical aggregation) |
| Multi-target fan-out | EnergyOpt -> HVAC + LIGHT + SHADE (broadcast setpoints) |
| Control/data co-flow | Setpoint is Control for sender, Data for receiver |
| Observe isolation | All observe ports reach only Collector, not delta computation |
| Safety override | FireSafety emergency_stop overrides all control commands |
| Feedback loop | HVAC -> EnergyOpt -> HVAC (legal: BoundedBuf edges, no algebraic loop) |
| Deployment invariance | Same Machine code, different LinkKinds for testing vs production |
| Type safety | Compiler prevents sending temperature to lighting controller |
| Runtime agnostic | Single-thread for-loop / Tokio / Embassy: zero code change |
