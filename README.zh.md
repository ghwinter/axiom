# axiom

[English](README.md) | 中文

**Func + Machine：类型化端口、显式拓扑、部署时物理决策。**

零依赖计算原语，构建可观测、可控制的软件系统。

`Func`（栈、无状态）与 `Machine`（堆、有状态）——配合类型化端口、显式
链接拓扑、部署规格、资源分类，以及代数基础。配套 `axiom-runtime`：把
`DeploySpec` 蓝图施工为可运行系统（单线程/多线程、融合、IO 多路复用）。

## 它是什么

```rust
use axiom::declare_ports;
use axiom::func::Func;
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::resource::MachinePhysicalSpec;
use axiom::deploy::{DeploySpec, MachineInstance};

// ── 纯函数：栈、无状态、可并行 ──
struct Scale;
impl Func for Scale {
    type Input = f64;
    type Output = f64;
    fn name() -> &'static str { "scale" }
    fn call(x: f64) -> f64 { x * 2.0 }
}

// ── 有状态机器：堆、持久、可观测 ──
declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AccumulatorPorts {
        input type AccumulatorInput {
            input [Data] => f64,
        }
        output type AccumulatorOutput {
            output [Data] => f64,
        }
    }
}

struct Accumulator;
impl Machine for Accumulator {
    type State = f64;
    type Input = AccumulatorInput;
    type Output = AccumulatorOutput;
    type Ports = AccumulatorPorts;
    type ProcessOutput = SingleOutput<AccumulatorOutput>;

    fn name() -> &'static str { "accumulator" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<f64, InitError> { Ok(0.0) }
    #[inline]
    fn process(
        s: &mut f64,
        _: &MachineContext,
        input: AccumulatorInput,
    ) -> SingleOutput<AccumulatorOutput> {
        let AccumulatorInput::input(x) = input;
        *s += x;
        SingleOutput::Yield(AccumulatorOutput::output(*s))
    }
    fn cleanup(s: f64, _: &MachineContext) -> Result<(), CleanupError> {
        println!("final: {s}");
        Ok(())
    }
}

// ── 声明拓扑（DeploySpec）──
let spec = DeploySpec::new()
    .with_machine(MachineInstance::new("acc", "accumulator", MachinePhysicalSpec::default()));

// ── 交给 runtime：axiom-runtime 施工蓝图 ──
// let mut rt = axiom_runtime::Runtime::new(RuntimeConfig::sequential());
// rt.register::<Accumulator>("accumulator");
// rt.materialize(&spec)?;
// rt.tick(vec![("acc".into(), "input".into(), Box::new(1.0f64))])?;
```

## 它不是什么

- 不是 runtime——`axiom` core 没有执行器、没有事件循环、没有线程（runtime 是 `axiom-runtime`）
- 不是框架——没有 Application trait、没有 main() 包装
- 不是纯抽象——它通过 `MachinePhysicalSpec` 共同定义物理接口

## 零成本抽象：两个存在层级

axiom 把 Rust 的零成本抽象原则扩展到架构层面。

任何 axiom 系统中存在两个独立的层级：

- **抽象层**——模块、端口、数据流、控制流、拓扑图。组织人类推理的数学对象。
- **物理层**——栈、CPU、指令集、地址。执行实际计算的物理实体。

两层互不相交。当我们说"模块 $M$ 向模块 $N$ 发送数据"时，物理上发生的是：
一个线程向某个地址写字节，另一个线程读取它们。"模块"与"发送"是语义标注；
物理层对它们一无所知。axiom 的职责是确保这些标注**不向物理层施加任何
运行时负担**。

### 两条执行路径

> **模型优先：DeploySpec 是任意图；static_path 是固定形状优化子集。**
> axiom 的默认模型是**任意有向图**（多进多出、fan-in、fan-out、环、复合嵌套）——
> 用 `DeploySpec` 声明、`validate_deep` 验证、runtime 执行。`static_path` 是
> **性能优化子集**：只覆盖编译期形状已知的拓扑（线性/扇形），因为它靠类型
> 展开（单态化）消解开销——任意图（尤其环）无法单态化，必须走动态路径。
> 线性不是 axiom 的场景假设；它是"类型展开"这一优化手段的固有边界。

| 路径 | 拓扑形状 | 拓扑确定时机 | 每消息成本 | 零成本? | 角色 |
|------|---------|-------------|-----------|---------|------|
| **DeploySpec + Runtime**（主模型） | **任意图**（环、fan-in/out、复合） | 运行时 | 有界（堆分配 + 分派） | 否（动态税不可避免） | 通用执行：复杂图系统 |
| **static_path**（优化子集） | 固定形状（线性/扇形/菱形） | 编译期 | **零** | 是 | 热路径：编译期已知形状 |

静态路径对具体机器类型单态化，并内联 `StraightLink::convert` /
`StraightSplit::split` / `StraightMerge::merge`——release 下编译产物等价于
直接手写批循环。动态路径必须通过 `Box<dyn Any>` 类型擦除，因为拓扑运行时
才可知；这种"动态税"数学上不可避免，不是实现缺陷。**两条路径都不对模型
施加线性假设**——任意图走动态路径；只有优化（单态化）受形状限制。

> **范围说明（反窄化规则）。** 静态执行路径
> （`axiom_runtime::static_path`）支持线性流水线（`pipeline2`/`pipeline3`、
> `pipeline_chain`）、扇出（`fanout2`，经 `Split`）、扇入（`fanin2`，经
> `Merge`）、菱形（`diamond`/`Diamond`，臂与下游可为任意链）。它无环
> （同步批模型）。**组合子**（`Chain`/`Diamond`/`feedback`）经
> `StraightMachine` 用裸载荷执行——无端口枚举标签、无运行时来源/去向验证
> （P0）：来源/去向由类型系统在编译期固定，因此路由错误是业务逻辑错误，
> 不是每消息的性能税。`Chain`（串行）与 `Diamond`（分叉-汇合）构成递归
> 代数，恰好生成**串并联 DAG**——流水线、map-reduce、菱形网络、多级分叉-
> 汇合树——全部单态化。真正的任意 DAG（含非串并联交叉边）超出此代数：
> 稳定 Rust 无法在保持端口类型安全的同时表达任意边表（边表是值级信息，
> 端口类型是类型级信息）。这类拓扑走动态路径（`Runtime`）；与动态税同理，
> 这是类型系统边界，不是实现缺陷。详见 `docs/philosophy.md` §"结构范围
> 约束"与 `docs/architecture.md` §"静态执行路径"。

**经验验证**（100k 消息 Transform → Sink 流水线，release 构建，单一参考环境）：

| 实现 | 相对吞吐 | vs 手写 |
|------|---------:|--------:|
| 手写（适配任务） | 1.0× | 基线 |
| 静态路径（单态化） | **1.24×** | 更快 |
| 动态路径（类型擦除） | 0.20× | 更慢 |

*相对比值（非绝对吞吐）：绝对值随机器/分配器变化；静态 > 手写 > 动态的
排序与环境无关。*

静态路径不仅匹配而且**超过**手写——抽象让编译器看到手写代码隐藏的结构，
使其能消除一个中间任务。形式化处理见 `docs/philosophy.md` 与
`docs/foundations.md` §15。

## 流语义：Data / Control / Observe

每个端口携带三种流类型之一（`axiom::flow::FlowKind`），链接的 FlowKind
必须匹配（`validate_deep` 拒绝不匹配）：

| 流 | 语义 | 丢失 | 副作用 |
|----|------|------|--------|
| `Data` | 模块处理的信息，改变状态内容 | 丢失 = 错误 | 改变状态 |
| `Control` | 改变行为/配置的指令 | 可丢弃（最新胜） | 改变行为 |
| `Observe` | 供外部消费的状态快照 | 可丢弃（尽力而为） | **不得**影响源 |

三向划分不是任意的：语义在*丢失容忍度*、*幂等性*、*副作用方向*上不同。
`Observe` 流保证不反向作用于其源——这让慢速观测模块可以用 `Dropping`
载体在自己的线程上运行而不阻塞主路径（经验验证，见下方 Showcase）。

## 内置模块

`Identity<I>`、`Sink<I>`、`Source<O>`、`Tee<I>`、`Latch<T>`、`Collector<I>`、
`EntityRoot`、`FuncMachine`

## 高级特性

| 特性 | 模块 | 描述 |
|------|------|------|
| **会话类型** | `axiom::session` | 二元 + 多方（MPST）协议，含 `GlobalType`/`LocalType` 投影、`is_dual`、`is_consistent` |
| **流式** | `axiom::stream` | `StreamingMachine`：拉模型迭代器输出（首次 `next()` 重置游标） |
| **借用输入** | `axiom::func` | `FuncRef::call_ref`：零拷贝输入（无每次调用分配） |
| **静态执行** | `axiom::static_exec` | `Chain`/`Diamond` 组合子 + `StraightMachine` 裸载荷直传（FusedInline 门控） |
| **动态拓扑** | `axiom::topology` | *实例*图的可选运行时变更（弹性伸缩、热替换、会话子图） |
| **混合系统** | `axiom::hybrid` | 经 `HybridMachine`（`flow`/`guard`/`reset`）的连续动力学，含 `TimeTick` 集成 |
| **生命周期类型状态** | `axiom::machine` | 经 `MachineHandle<M, S>` 编译期强制 `Init → Running → Stopping → Stopped` |
| **复合机器** | `axiom::composite` | `CompositeSpec` + `expand_composites`：子系统嵌套（递归、深度受限） |
| **AI 蓝图** | `axiom::blueprint` *(serialize)* | `DeploySpec` 的 JSON Schema 导出 + 严格反向解析器：AI 写 JSON，得结构化错误，迭代 |
| **结构化验证** | `axiom::deploy` | `validate_report`：收集**全部**违规为 `RuleViolation {rule_id, path, expected, actual}`（非 fail-fast） |
| **架构 lint** | `axiom::lint` | 反窄化公理作为可执行规则：`no-observation`、`default-physical`、`uniform-link-kind` 等 |
| **运行时契约** | `axiom::runtime_contract` | `RuntimeContract` trait + `Guarantees`（链接载体/执行模式/内存序/IO/延迟）——审计蓝图 vs 适配器物理能力 |

### 静态优先世界观

axiom 的默认是**静态拓扑**：`DeploySpec` 声明一次、验证一次
（`validate_deep`），系统运行期间不变。静态拓扑零成本（单态化的
`static_path` 函数）且部署前完全可分析（反馈环、SPOF、度约束、Inline
无环性）。运行时拓扑变更是少数需要它的系统的*可选*能力——弹性伸缩
（已有机器类型的副本）、热替换（原位升级机器）、会话子图（形状编译期
固定但实例运行期创建/销毁的协议）。实例图可以移动；类型空间是静态的。
完整理由与三个合法动态拓扑用例见 `docs/philosophy.md` §"静态优先世界观"。

## axiom-runtime

`Runtime` 以显式物理执行 `DeploySpec`：

- **执行模式**：`Inline` / `Sequential`（BFS 直接投递）/ `Parallel(n)`（每机器一线程，channel 载体）
- **载体矩阵**：`Blocking`（背压）/ `Dropping`（丢新）/ `Overwriting`（环形覆盖）/ `Latest`-`SharedState`（单槽）——`LinkKind` 的*物理实现*
- **生命周期**：`Done` 是停机信号——向下游传播（级联停机）、丢弃积压、并行线程退出
- **融合**：FusedInline 链上的 pipelineN 融合（减少每跳分配）
- **并行环**：Kahn 环检测 + `stop_signal` 终止
- **IO 多路复用**：`IoReactor` trait——epoll / kqueue / WSAEventSelect 后端 + `ManualReactor`、`default_reactor()`
- **观测/调试**：`Observe` 流监控（独立线程、`Dropping` 载体）+ `Control` 流反向注入
- **确定性**：同输入序列 → 同输出（Sequential/Parallel 跨模式验证、replay）

## 示例

Core（`examples/`）：

| 示例 | 演示 |
|------|------|
| `http_tutorial` | 入门路径：Receiver → Calculator → Persister + ASCII 拓扑 |
| `threaded_pipeline` | Source → Tee → 2×Worker → Collector，多线程契约压力 |
| `psql` | SQL REPL 流水线（lexer/parser/executor 作为 `Func`/`FuncRef`），`--bench` 分配统计 |
| `declarative_dag` | 复合 + 多 LinkKind 声明式验收 |
| `graph_validation` | **复杂图验证与分析**：内核风格图（syscall 扇出 + 双路径 + 3 反馈环 + 观测）通过 `validate_deep`；逐项检出流类型不匹配 / Inline 环 / 全非 Moore 环；SPOF / 环 / 度 / 可达性分析报告 |

Runtime（`runtime/examples/`）：

| 示例 | 演示 | 验证 |
|------|------|------|
| `http_declarative` | 同一拓扑，声明式 `register → materialize → tick` | Sequential == Parallel |
| `redis_like` | 6 模块服务器蓝图：gateway → RESP → KV/List/Hash → encoder → writer + AOF；**monitor (Observe) + debugger (Control)** | `--replay` 确定性；`--bench` 载体效应 |
| `mmo` | MMO 核心子图：sessions（生命周期 + 心跳超时）、world 分片、每玩家视图投影、事件溯源 | `--replay` 事件流确定性 |
| `netpath` | 内核网络 RX 路径：pcap → Ethernet → IP → TCP → deliver + 统计观测者 | `--replay` 字节一致双重回放 |
| `composite_machine` | 运行时 CompositeSpec 展开 | — |
| `bench_runtime` | 运行时开销统计 | — |

**Showcase：观测不得拖慢主路径**（`redis_like --bench`，Parallel(4)，monitor 模拟 20µs/事件）：

| 配置 | 主路径吞吐（相对） |
|------|------------------:|
| 基线（无 monitor） | 1.0× |
| monitor + **Blocking** | **0.2×（-80%）**——观测阻塞主路径 |
| monitor + **Dropping** | **≈1.0×**——观测被丢弃，主路径干净 |

*单一参考环境的相对比值；绝对 cmd/s 随机器变化。模式（Blocking 阻塞、
Dropping 不阻塞）与环境无关。*

慢速观测模块用 `Dropping` 载体在独立线程上运行不阻塞主路径；`Blocking`
会。这是"低速行为住在独立线程；蓝图不规定物理，载体选择规定"的经验陈述。

## 测试

- `axiom` core：**314 测试**（209 unit + 105 集成，含源码审计）+ 21 doctests——全绿
- `axiom-runtime`：**85 测试**——全绿
- 验证哲学：证据语料 `evidence/`（E-contracts + R-benchmarks，仅本地，不入 git）

## 深入阅读

| 文档 | 覆盖 |
|------|------|
| [`docs/foundations.md`](docs/foundations.md) | 代数基础——公理、定理、证明 |
| [`docs/philosophy.md`](docs/philosophy.md) | 设计哲学——抽象 vs 物理、控制/数据模糊 |
| [`docs/design-principles.md`](docs/design-principles.md) | 元问题与设计原则——零成本作为形态同构、验证判据、物理作为有限选择集合 |
| [`docs/doc-governance.md`](docs/doc-governance.md) | 文档标准与决策记录——分层、字数预算、决策日志 |
| [`docs/adapters.md`](docs/adapters.md) | Adapter 生态规则与运行时契约认证——Guarantees 审计、发布分层 |
| [`docs/architecture.md`](docs/architecture.md) | 架构细节——端口、链接、部署、runtime 对比 |
| [`docs/architecture_diagrams.md`](docs/architecture_diagrams.md) | 图——系统分层、链接策略、部署、路线图 |

## 为什么叫 "axiom"

Axiom 是不证自明、作为基础的真理。`Func` 与 `Machine` 是计算组织的公理。
其余一切皆为派生。
