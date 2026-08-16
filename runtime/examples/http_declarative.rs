//! http_declarative — `http_tutorial` 拓扑的**声明式验收**。
//!
//! 同一张图（Receiver → Calculator → Persister），这次不再手写
//! `MachineHandle` 驱动循环，而是交给 `axiom-runtime`：
//!
//! ```text
//!   register 三个机器类型 ──► materialize(DeploySpec) ──► tick(输入序列)
//! ```
//!
//! 验证：
//! 1. `Sequential` 模式：注入 3 个请求，终端输出 = 3 条 status
//!    （`balance=10/5/8`）——Calculator 的 status 观察端口无下游，
//!    被收集为终端输出；balance 数据端口路由到 Persister。
//! 2. `Parallel(4)` 模式：同一 spec 产出相同结果（R001 确定性）。
//!
//! 运行：cargo run --manifest-path runtime/Cargo.toml --example http_declarative

use axiom::declare_ports;
use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput, TupleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::resource::MachinePhysicalSpec;
use axiom_runtime::{ProcessResult, Runtime, RuntimeConfig};

// ════════════════════════════════════════════════════════════════════════
// 数据类型（与 axiom 的 examples/http_tutorial.rs 相同）
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    delta: i64,
    src: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    delta: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    value: i64,
}

// ════════════════════════════════════════════════════════════════════════
// 模块 1：Receiver —— 接收 + 解析
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ReceiverPorts {
        input type ReceiverInput {
            raw [Data] => RawRequest,
        }
        output type ReceiverOutput {
            parsed [Data] => ParsedRequest,
        }
    }
}

pub struct Receiver;

impl Machine for Receiver {
    type State = u64;
    type Input = ReceiverInput;
    type Output = ReceiverOutput;
    type Ports = ReceiverPorts;
    type ProcessOutput = SingleOutput<ReceiverOutput>;

    fn name() -> &'static str { "receiver" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
    #[inline]
    fn process(
        state: &mut u64,
        _: &MachineContext,
        input: ReceiverInput,
    ) -> SingleOutput<ReceiverOutput> {
        let ReceiverInput::raw(req) = input;
        *state += 1;
        SingleOutput::Yield(ReceiverOutput::parsed(ParsedRequest { delta: req.delta }))
    }
    fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 2：Calculator —— 核心逻辑，状态随数据变化
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct CalculatorPorts {
        input type CalculatorInput {
            apply [Data] => ParsedRequest,
        }
        output type CalculatorOutput {
            balance [Data]    => Balance,
            status  [Observe] => String,
        }
    }
}

pub struct Calculator;

impl Machine for Calculator {
    type State = i64;
    type Input = CalculatorInput;
    type Output = CalculatorOutput;
    type Ports = CalculatorPorts;
    type ProcessOutput = TupleOutput<CalculatorOutput>;

    fn name() -> &'static str { "calculator" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, InitError> { Ok(0) }
    #[inline]
    fn process(
        state: &mut i64,
        _: &MachineContext,
        input: CalculatorInput,
    ) -> TupleOutput<CalculatorOutput> {
        let CalculatorInput::apply(req) = input;
        *state += req.delta;
        TupleOutput::Yield(
            CalculatorOutput::balance(Balance { value: *state }),
            CalculatorOutput::status(format!("balance={}", *state)),
        )
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 3：Persister —— 持久化（内存历史）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PersisterPorts {
        input type PersisterInput {
            save [Data] => Balance,
        }
        output type PersisterOutput {
            // 纯汇：无输出端口
        }
    }
}

pub struct Persister;

impl Machine for Persister {
    type State = Vec<i64>;
    type Input = PersisterInput;
    type Output = PersisterOutput;
    type Ports = PersisterPorts;
    type ProcessOutput = SingleOutput<PersisterOutput>;

    fn name() -> &'static str { "persister" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<Vec<i64>, InitError> { Ok(Vec::new()) }
    #[inline]
    fn process(
        state: &mut Vec<i64>,
        _: &MachineContext,
        input: PersisterInput,
    ) -> SingleOutput<PersisterOutput> {
        let PersisterInput::save(b) = input;
        state.push(b.value);
        SingleOutput::Idle
    }
    fn cleanup(_: Vec<i64>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 拓扑 + 驱动
// ════════════════════════════════════════════════════════════════════════

fn topology() -> DeploySpec {
    DeploySpec::new()
        .with_machine(MachineInstance::new("receiver", "receiver", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("calc", "calculator", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("persist", "persister", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("receiver", "parsed"),
            ("calc", "apply"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("calc", "balance"),
            ("persist", "save"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ))
}

fn run(cfg: RuntimeConfig) -> Vec<String> {
    let mut rt = Runtime::new(cfg);
    rt.register::<Receiver>("receiver");
    rt.register::<Calculator>("calculator");
    rt.register::<Persister>("persister");
    rt.materialize(&topology()).expect("materialize");

    let requests = vec![
        RawRequest { delta: 10, src: "client-1".into() },
        RawRequest { delta: -5, src: "client-2".into() },
        RawRequest { delta: 3, src: "client-1".into() },
    ];
    let inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)> = requests
        .into_iter()
        .map(|r| ("receiver".to_string(), "raw".to_string(), Box::new(r) as Box<dyn core::any::Any + Send>))
        .collect();

    let out = rt.tick(inputs).expect("tick");
    // 终端输出 = status 观察端口（无下游）；balance 路由到 Persister（Idle）。
    out.into_iter()
        .filter_map(|r| match r {
            ProcessResult::Yield { value, .. } => value.downcast::<String>().ok().map(|b| *b),
            _ => None,
        })
        .collect()
}

fn main() {
    let seq = run(RuntimeConfig::sequential());
    let par = run(RuntimeConfig::parallel(4));

    println!("sequential: {:?}", seq);
    println!("parallel(4): {:?}", par);

    let expected = vec!["balance=10", "balance=5", "balance=8"];
    assert_eq!(seq, expected, "Sequential must yield the 3 statuses in order");
    assert_eq!(par, expected, "Parallel(4) must yield the same statuses (R001 determinism)");

    println!("✓ http_tutorial declared declaratively: Sequential == Parallel, statuses correct");
}
