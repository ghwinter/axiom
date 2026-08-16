//! axiom 第一个应用：一个极简 HTTP 服务器（教学用例）。
//!
//! # 图结构（抽象层 —— 语义拓扑）
//!
//! ```text
//!      ┌────────────────────┐  parsed   ┌─────────────────────┐  balance  ┌──────────────────┐
//!      │      Receiver      │ (Data)    │     Calculator      │ (Data)   │     Persister    │
//! raw  │  State: u64        │──────────►│  State: i64         │─────────►│  State: Vec<i64> │
//! ────►│  ┌raw [Data]───┐   │           │  ┌apply [Data]───┐  │          │  ┌save [Data]───┐ │
//!      │  └►parsed[Data]┘   │           │  └►balance[Data]┘  │          │  └►（历史快照）   │ │
//!      └────────────────────┘           │   status[Observe]──┼──┐       └──────────────────┘
//!                                       └─────────────────────┘  │
//!                                                                ▼
//!                                                      [ 日志 / 监控 ]
//!                                                        （观察端口）
//!
//! 链接（LinkKind）：两条边都是 BoundedBuf{cap:16, Blocking} ——
//! 声明"有界缓冲 + 背压"的物理语义（部署者可换成 Inline / Channel，
//! 三个模块的代码一个字都不用改）。
//! ```
//!
//! # 物理过程（手写驱动 = 最小 runtime，单线程顺序执行）
//!
//! ```text
//!  main 线程（唯一线程 —— 无锁、无通道，链接退化为直接函数调用）
//!
//!  ┌──────────────────────────────────────────────────────────────────────┐
//!  │ ① Receiver::process(raw)       ← 栈帧；State(u64) 在堆             │
//!  │ ② Calculator::process(parsed)  ← 栈帧；State(i64) 在堆，余额 += δ   │
//!  │ ③ Persister::process(balance)  ← 栈帧；State(Vec<i64>) 在堆，push  │
//!  └──────────────────────────────────────────────────────────────────────┘
//!
//!  数据移动（物理载体）：
//!   RawRequest ─move→ Receiver 栈 ─move→ ParsedRequest（栈上构造）
//!   ParsedRequest ─move→ Calculator 栈 ─move→ Balance（栈上构造）
//!   Balance ─move→ Persister 栈 ─push→ Vec<i64>（堆，可能 realloc）
//!
//!  注意：BoundedBuf 链接在单线程驱动下"物化"为直接 move —— 没有缓冲区、
//!  没有锁 —— 与 Inline 链接的物理展开一致（抽象消解：图上有一条边，
//!  物理上只是函数调用）。若未来 runtime 把 Receiver 与 Calculator 放在
//!  不同线程，BoundedBuf 才物化为真实的环形缓冲 + 锁。
//! ```
//!
//! 三个模块各是一个 `Machine`：
//! - **Receiver**：接收原始请求、解析出操作数（记接收数，无业务状态）
//! - **Calculator**：核心计算，持有累计余额状态；状态随数据变化，
//!   变化通过 balance 端口驱动下游持久化
//! - **Persister**：把每次余额快照持久化（内存历史；换磁盘只改这一处）
//!
//! axiom core 不带 runtime，因此这里手写了一个最小驱动（每个 Machine
//! 一个 handle，顺序 process）——它就是未来 runtime adapter 会从
//! `DeploySpec` 自动生成的代码的最小等价物。末尾的 `DeploySpec` 声明了
//! 与驱动完全相同的拓扑（纯数据、可序列化、可验证）。

use axiom::declare_ports;
use axiom::machine::{
    CleanupError, InitError, Machine, MachineHandle, SingleOutput, TupleOutput, Init,
};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::prelude_all::*; // DeploySpec / MachineInstance / LinkSpec / LinkKind 等

// ════════════════════════════════════════════════════════════════════════
// 数据类型 —— 在模块之间流动的消息
// ════════════════════════════════════════════════════════════════════════

/// 模拟从 socket 读到的原始请求（真实世界会是字节流 + 解析）。
#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    delta: i64,
    src: String,
}

/// Receiver 解析后的请求：只保留操作数。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    delta: i64,
}

/// Calculator 产出的余额快照。
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
    type State = u64; // 接收计数
    type Input = ReceiverInput;
    type Output = ReceiverOutput;
    type Ports = ReceiverPorts;
    type ProcessOutput = SingleOutput<ReceiverOutput>; // 1:1 机器

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
        // 模拟协议解析：丢弃 src，只提取操作数
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
            balance [Data]    => Balance, // 数据端口：驱动下游持久化
            status  [Observe] => String,  // 观察端口：给日志/监控
        }
    }
}

pub struct Calculator;

impl Machine for Calculator {
    type State = i64; // 累计余额 —— 状态随数据变化
    type Input = CalculatorInput;
    type Output = CalculatorOutput;
    type Ports = CalculatorPorts;
    // 数据 + 观察各产出一个：固定双输出（TupleOutput），可进融合流水线
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
        *state += req.delta; // 状态变化
        TupleOutput::Yield(
            CalculatorOutput::balance(Balance { value: *state }),
            CalculatorOutput::status(format!("balance={}", *state)),
        )
    }

    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 3：Persister —— 持久化（内存历史；换磁盘只改这一处）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PersisterPorts {
        input type PersisterInput {
            save [Data] => Balance,
        }
        output type PersisterOutput {
            // 无输出端口 —— 纯汇
        }
    }
}

pub struct Persister;

impl Machine for Persister {
    type State = Vec<i64>; // 历史快照（模拟磁盘持久化）
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
        // 真实实现：append 到文件 / 写 WAL。这里用内存历史。
        state.push(b.value);
        SingleOutput::Idle // 汇：无输出
    }

    fn cleanup(_: Vec<i64>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 拓扑声明（DeploySpec）—— 纯数据，与下面的手写驱动等价
// ════════════════════════════════════════════════════════════════════════

fn declare_topology() -> DeploySpec {
    DeploySpec::new()
        .with_machine(MachineInstance::new(
            "receiver", "receiver", MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "calc", "calculator", MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "persist", "persister", MachinePhysicalSpec::default(),
        ))
        .with_link(LinkSpec::new(
            ("receiver", "parsed"),
            ("calc", "apply"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("calc", "balance"),
            ("persist", "save"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}

// ════════════════════════════════════════════════════════════════════════
// main —— 手写最小驱动（runtime 的雏形）
// ════════════════════════════════════════════════════════════════════════

fn main() {
    // 拓扑声明：当前 core 无 runtime，它描述"图长什么样"；
    // 未来的 runtime adapter 会按它物化出下面的驱动代码。
    let _spec = declare_topology();

    // 物化：每个 Machine 一个 handle（typestate：Init → Running）
    let mut receiver = MachineHandle::<Receiver, Init>::new(MachineContext::new("receiver"))
        .expect("receiver init")
        .start();
    let mut calculator = MachineHandle::<Calculator, Init>::new(MachineContext::new("calc"))
        .expect("calculator init")
        .start();
    let mut persister = MachineHandle::<Persister, Init>::new(MachineContext::new("persist"))
        .expect("persister init")
        .start();

    // 模拟 HTTP 请求流：/add/10, /add/-5, /add/3
    let requests = vec![
        RawRequest { delta: 10, src: "client-1".into() },
        RawRequest { delta: -5, src: "client-2".into() },
        RawRequest { delta: 3, src: "client-1".into() },
    ];

    for req in requests {
        // Receiver：raw → parsed（手写驱动扮演 runtime 的链接投递）
        let ReceiverOutput::parsed(parsed) = match receiver.process(ReceiverInput::raw(req)) {
            SingleOutput::Yield(o) => o,
            _ => unreachable!(),
        };

        // Calculator：apply → (balance, status)
        let out = calculator.process(CalculatorInput::apply(parsed));
        let (balance, status) = match out {
            TupleOutput::Yield(a, b) => (a, b),
            _ => unreachable!(),
        };

        // status 观察端口 → 日志；balance 数据端口 → Persister
        let s = match status {
            CalculatorOutput::status(s) => s,
            _ => unreachable!(),
        };
        println!("[log] {}", s);

        let b = match balance {
            CalculatorOutput::balance(b) => b,
            _ => unreachable!(),
        };
        let _ = persister.process(PersisterInput::save(b));
    }

    // 优雅停机前取出持久化历史（typestate 允许 Running 读 state）
    let history: Vec<i64> = persister.state().clone();

    // 优雅停机（typestate：Running → Stopping → Stopped → cleanup）
    let receiver = receiver.stop().finish();
    let calculator = calculator.stop().finish();
    let persister = persister.stop().finish();
    receiver.cleanup().expect("receiver cleanup");
    calculator.cleanup().expect("calculator cleanup");
    persister.cleanup().expect("persister cleanup");

    // 验证持久化内容
    println!("persisted history: {:?}", history);
    println!("expected          : [10, 5, 8]");
    assert_eq!(history, vec![10, 5, 8]);
}
