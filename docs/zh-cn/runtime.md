> **语言：** 中文 · [English](../en-us/runtime.md)

# axiom runtime：物理层实现用例（载体 Carrier）

> **性质**：axiom 的**物理层架构规范**。回答"axiom 的物理层应该是什么"：核心
> `cell_core` 只声明**因果数据流**（`A.out -> B.in`），runtime 回答唯一一个问题——
> **这条流的值怎么从 `A.out` 到 `B.in`，以何种时空成本**。它描述 runtime 的应有
> 形态，与已收敛的实现（`runtime/src/{carrier,flow,static_path,macros,lib}.rs`）一致。
>
> **规范性**：本卷是自洽的权威规范，专注 axiom 物理层自身的形态。
>
> **定位（一句话）**：runtime = **载体（Carrier）目录 + 兑现验证**：它为 `cell_core`
> 的每条因果数据流提供一种物理实现（值怎么移动），每种体现不同的时空成本，模块化、
> 可替换。runtime 是核心的**物理层实现用例**——axiom 无运行时对象，只有"编译期"与
> "编译后"两段。

---

## 1. 概念基础（源于 cell_core）

- `cell_core`：开放系统（`PortCell`: In/Out/State/step）+ 因果流（`Wire`/`Chain`/
  `Broadcast`/`Merge`/`Feedback`）+ 静态性（`Static`）+ 编译期验证（统一 `Conforms`）。
- 蓝图即类型：零大小、零运行时对象、编译期耗尽。
- **runtime 不重复这些**——它只回答"这条因果流，值怎么从 A.out 到 B.in"。

---

## 2. 核心抽象：`Carrier`

```rust
pub enum CarrierCost { ZeroAllocInline, PerMessageAlloc, External }   // 时空成本声明

pub trait Carrier<A, B>
where A: PortCell, B: PortCell<In = A::Out>,   // T1：因果流本身合法
{
    fn cost() -> CarrierCost { CarrierCost::ZeroAllocInline }
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out;
}
```

载体目录（`runtime/src/carrier.rs`）：

| 载体 | 物理方案 | 时空成本 | 线程 | 模块 |
|---|---|---|---|---|
| `InlineCarrier` | 栈上函数直接传（`B::step(A::step(x))`） | 零分配、内联 | 单线程 | carrier.rs |
| `DirectCarrier` | 编译期展开标记（`static_path` 兑现） | 零运行时对象 | 单线程 | carrier.rs |
| `QueueCarrier`（std） | 堆队列中转（`Box<dyn Any>` 每消息分配） | 每消息分配 | 单线程内 | carrier.rs |
| `ChannelCarrier` / `spawned_flow`（std） | mpsc 通道 + 独立线程，`B::State` 在专用线程 | 每消息分配 + 同步 | **跨线程** | carrier.rs |

每种载体**独立可选、可替换**：换一个实现不改拓扑（T6 多物理实现）。

> **载体即属性（部署期物理）**：蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>`
> 走 `InlineCarrier`/`static_path`），runtime 按声明兑现。"丢弃/阻塞/同步/异步"全是
> 物理层选择（衔接 `foundations.md` §5.8）——同一蓝图换载体即换"丢弃/阻塞/同步"行为。

---

## 3. 驱动（flow）与静态路径（static_path）

- **`flow`**（`runtime/src/flow.rs`）：`drive_link` / `drive_wired`——编译期布线验证
  （统一 `Conforms<Wire>`）后，用选定载体驱动一条 A→B 因果流；**验证在编译期，运行期零开销**。
- **`static_path`**（`runtime/src/static_path.rs`）：`run_static` / `run_declared_static`——
  把被 `Static<SUB>` 声明为"要求零成本"的子图在**编译期内联展开**（零运行时对象）。
- **声明宏**（`runtime/src/macros.rs`）：`wire!`——编译期展开的"连线 + 载体 + 验证"
  一次完成的宏/编译期技巧。

---

## 3b. 统一模型激活（runtime，`std`）

runtime 为统一模型构造子（在 `core.md` 中是**定义**；激活仍是运行/载体侧）提供**激活**：

- **`SlotDrive<I, O>`**（`runtime/src/slot.rs`）——对 `Slot<I,O>` 的 ∃ 存在化填充：安装一个
  编译期合规占用者（`T: PortCell<In=I,Out=O>` ⟹ core 的 `Conforms`）、把其状态类型擦除为
  `Box<dyn Any + Send>`、运行期 `drive`/`swap`。这是"动态装载"的物理侧——接口固定且编译期
  T1 验证、占用者运行期存在化。
- **`drive_seq`**（`runtime/src/flow.rs`，`std`）——`Rep<N,C>` 的生成/无界计数侧：把一组运行期
  `IntoIterator` 输入依次流经同一 cell，收集输出，状态跨次保持（计数由运行期决定，非编译期）。

二者 `std` 门控、安全（`#![forbid(unsafe_code)]`）；把动态税局部化到缝上（见
[`unified.md`](unified.md) §5）。

## 4. 模块化与可替换（可扩展的物理载体）

- 每个 carrier 是**独立单元**，可作为单独 crate。
- 新的物理载体通过实现 `Carrier` trait 挂入，**不改 cell 拓扑**：例如用带其他调度/
  时序语义的通道载体替换 `ChannelCarrier`，或用其他底层机制替换零分配载体。
- runtime 作为参考实现用例，提供各载体作模板。

---

## 5. 语义等价与验证（可替换性的契约）

- **多物理实现语义等价**（T6）：任何载体 `flow` 语义上等价于 `B::step(sb, A::step(sa, x))`
  ——即同一条因果流。换载体不改输出。
- **确定性与等价性验收**：真实用例（见 §7）在 Inline 与 Queue 等载体上跑出逐位一致的输出，
  并验证确定性（同输入重跑同输出）。这应作为 **carrier 语义等价回归验收**——凡新增载体，
  须先在既有用例上断言输出一致，防止某载体悄悄破坏语义（这是工程守则，源自 netpath 实操）。
- **编译期 vs 运行时验证**：布线合法性与静态性验证在编译期（T1/统一 Conforms）；载体的
  时空成本是可选的量化声明（`CarrierCost`），非性能承诺。

---

## 6. 构建与验收基准

```text
cargo build/test --manifest-path runtime/Cargo.toml   # runtime（7 测试）
cargo run --manifest-path runtime/Cargo.toml --example carrier_demo
cargo run --manifest-path runtime/Cargo.toml --example threaded_flow
cargo bench --manifest-path runtime/Cargo.toml --bench carrier
```

**已达成（证据链）**：
- runtime 只依赖 cell_core（新核心），不依赖任何 v0 模块。
- 载体目录：Inline（栈上函数传·零分配）/ Queue（堆队列中转）/ Channel（跨线程 mpsc）/
  Direct+static_path（编译期展开）/ wire!（声明宏）。
- 模块化可替换：换载体不改拓扑（T6），各载体独立可单独引用。
- `#![forbid(unsafe_code)]`、no_std（`std` feature 门控非零分配/跨线程载体）。
- bench：Inline 2.7µs vs spawned_flow 6.96s——同一因果流在单线程内联与跨线程通道载体下的
  时空成本差异，实证"不同载体 = 不同时空成本"。

---

## 7. 真实用例（runtime 作为核心的实现用例）

| 用例 | 类型 | 驱动的 runtime 能力 |
|---|---|---|
| `redis_like` | 多模块服务器类 | 多模块管线 + 单线程/跨线程（spawned_flow） |
| `psql` | 解析/执行流水线类 | 流水线组合 + Inline/跨线程解析 |
| `mmo` | 多人世界/广播 | Broadcast 多对多 fan-out + 世界状态 + 视图投影 |
| `netpath` | 多段网络管线 | 多段解析组合 + Queue vs Inline 载体等价 + 确定性 |
| `carrier_demo` | 载体演示 | 同一蓝图多载体可替换、语义等价、时空成本不同 |
| `threaded_flow` | 同拓扑异构物理 | Inline 零分配 vs 跨线程通道 |

这些用例是"基于 axiom/axiom-runtime 构建真实程序"的构建用例，也是 runtime 迭代与
等价性验证的载体。旧版同类用例（含 TCP 服务器形态）可在 git 历史中恢复作参考
（`git show main:runtime/examples/<name>/main.rs`）。

---

## 8. 边界（诚实声明）

- 这是**物理层实现用例 + 模板**，不是"功能最全的通用 runtime"。
- **N2N 属于物理层**：多对多的并行调度、队列仲裁、借用、缓存、线程属于物理实现的
  领域——axiom 不重造，axiom 提供"载体契约"（声明偏好）与"兑现验证"，让多对多
  实现可替换地挂入。
- **动态税不可消除且正当**：当且仅当结构必须在运行期确定（配置/插件/动态拓扑）时付税
  （衔接 T7/T9）；否则必须走静态路径。

---

## 9. 已知开放边界（诚实记录）

> 下列是 runtime 定位内的**薄边**，由真实用例暴露，当前**未解决但已如实记录**。它们
> 属于"工程叠加/优化 + 一处理论边界"，不改核心（`cell_core`）的既有构成。

### 9.1 背压 / 有界缓冲
当前 `QueueCarrier`/`ChannelCarrier` 是**无界** mpsc 形态；真实系统需要"生产快而消费慢"
的有界 + 背压语义。
- 落层：**纯 runtime**（`foundations.md` 已把"背压/时序"归物理载体）。
- **已提供**：
  - `BoundedQueue<T, CAP>`（`buffer.rs`，std）——基于 `sync_channel(CAP)` 的有界 FIFO：
    `push`（阻塞=背压）、`try_push`（满返回 `Err`=背压/容量信号，丢弃/上抛策略留给调用侧）；
  - `BoundedCarrier<CAP>`（`carrier.rs`）——有界通道物理形态的 `Carrier`（`CAP` 编译期常量、
    `PerMessageAlloc` 成本）；
  - `bounded_pump<A,B,It,CAP>`（`flow.rs`）——**真实阻塞背压**：生产端把 `A` 的输出投入容量
    `CAP` 的有界队列，满时生产端阻塞，直到消费者线程 drain；返回 `B::step` 输出序列。

### 9.2 错误 / 失败通路（理论–实际偏差）
真实 cell（如解析器）需要 "会失败" 的语义；`PortCell::step` 被假定为**总转移**
（`foundations.md` 边界已如实标注：全函数假设）。目前以 `Out = Result` 约定 + 短路载体
拼凑。
- 落层：**可归 runtime**（用 `Result` 约定 + 短路），与"丢弃/阻塞是物理"同构；若将
  "会失败的 cell"公理化（部分函数/错误输出端口），则属理论边界（`foundations.md` §7
  开放问题 5）。
- **已提供（短路侧）**：
  - `drive_try<A,B,X,E>`——把产出 `Result` 的 `A` 连到消费其 `Ok` 的 `B`，遇 `Err` 立即短路
    （`no_std` 安全）；
  - `TryChain<A,B>`——两个会失败的 cell 的可组合短链：整条 fallible 流水线是一个单层
    `Result` 的 `PortCell`（比 `drive_try` 的嵌套 `Out` 更干净；psql 用
    `TryChain<TryChain<Lexer,Parser>,Executor>` 表达完整 REPL）。
- 仍剩：一等短路载体（如 `MaybeCarrier`/`ResultCarrier`）、失败×背压的联合语义。

### 9.3 外部输入源的接入接缝（IO 事件 ↔ flow）
文档已声明"IO 是物理/载体可替换"，但"外部世界（socket 事件等）如何正式成为一条因果流
的 `in`"这一落地接口未形式化。redis_like 升级为真实 TCP（监听/accept/按帧解析）即此接缝
的第一个实现用例。
- 落层：**runtime**（事件基座即一类载体/驱动）。
- 方向：提炼事件基座（event-substrate），使外部事件成为一类可替换的输入载体/驱动。

### 9.4 失败 × 背压同时发生
缓冲满与处理失败同时出现时的语义未被规定——留待上述二者定形后一并收敛。

---

## 10. 结论

> runtime = `cell_core` 的**物理层实现用例**：载体目录（Inline/Queue/Channel/Direct/
> static_path/wire!）+ 兑现验证，模块化可替换，解释并验证"同一张静态图可插进多种物理
> 执行（内联/队列/跨线程），每种有可验证的语义等价"。载体可通过实现 `Carrier` trait
> 挂入而不改拓扑，使物理层具备可扩展性；其边界内的开放问题（背压、错误通路、
> IO 接缝）已如实记录，作为后续迭代的驱动输入。
