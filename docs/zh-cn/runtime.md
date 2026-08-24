> **语言：** 中文 · [English](../en-us/runtime.md)

# axiom runtime：物理层实现用例（载体 Carrier）

> **性质**：axiom 的**物理层架构规范**。回答"axiom 的物理层是什么"：核心
> `cell_core` 只声明**因果数据流**（`A.out -> B.in`），runtime 回答唯一一个问题——
> **这条流的值怎么从 `A.out` 到 `B.in`，以何种时空成本**。本卷描述 runtime 的形态，
> 与已收敛的实现（`runtime/src/{carrier,flow,static_path,macros,contract,slot,buffer,lib}.rs`）一致。
>
> **规范性**：自洽的权威规范，专注 axiom 物理层自身的形态。
>
> **定位（一句话）**：runtime = **载体（Carrier）目录 + 兑现验证**：为 `cell_core`
> 的每条因果数据流提供一种物理实现（值怎么移动），每种体现不同的时空成本，模块化、
> 可替换。runtime 是核心的**物理层实现用例**——axiom 无运行时对象，只有"编译期"与
> "编译后"两段。

---

## 1. 概念基础（源于 cell_core）

- `cell_core`：开放系统（`PortCell`: In/Out/State/step）+ 因果流（`Wire`/`Chain`/
  `Broadcast`/`Merge`/`Feedback`）+ 静态性（`Static`）+ 编译期验证（统一 `Conforms`）。
- 蓝图即类型：零大小、零运行时对象、编译期耗尽。
- **runtime 不重复核心的声明**——runtime 只回答"这条因果流，值怎么从 A.out 到 B.in"。

---

## 2. 核心抽象：`Carrier`

```rust
pub enum CarrierCost { ZeroAllocInline, PerMessageAlloc, External }   // 时空成本声明

pub trait Carrier<A, B>
where A: PortCell, B: PortCell<In = A::Out>,   // T1：因果流本身合法
{
    fn cost() -> CarrierCost { CarrierCost::External }
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out;
}
```

载体目录（`runtime/src/carrier.rs`）：

| 载体 | 物理方案 | 时空成本 | 线程 | 模块 |
|---|---|---|---|---|
| `InlineCarrier` | 栈上函数直接传（`B::step(A::step(x))`）；编译期展开（Direct 已并入） | 零分配、内联 | 单线程 | carrier.rs |
| `QueueCarrier`（std） | 堆队列中转（`Box<dyn Any>` 每消息分配） | 每消息分配 | 单线程内 | carrier.rs |
| `BoundedCarrier<CAP>`（std） | 有界通道中转（`CAP >= 1` 编译期强制） | 每消息分配 | 单线程内 | carrier.rs |
| `spawned_flow`（std） | mpsc 通道 + 独立线程，`B::State` 在专用线程；worker panic 经回执传播 | 每消息分配 + 同步 | **跨线程** | carrier.rs |

每种载体**独立可选、可替换**：换一个实现不改拓扑（T6 多物理实现）。

> **放置连续谱（衔接 `foundations.md` §8.6 第 7–8 条）**：表中"单线程 / 跨线程"**不是两个
> 模型，是同一物理放置决策谱系的两端**——同一张蓝图经放置决定每条边在谱上的位置。表内各
> 载体是谱上不同位置的物理形态：单线程载体 = "所有边同线程放置"的原生形态（族 A = 0），
> 跨线程载体如实承担族 A（并发维持对价）。零成本承诺（族 B = 0，见下）对二者同等成立。

> **载体即属性（部署期物理）**：蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>`
> 走 `InlineCarrier`/`static_path`），runtime 按声明兑现。"丢弃/阻塞/同步/异步"全是
> 物理层选择（衔接 `foundations.md` §5.8）——同一蓝图换载体即换"丢弃/阻塞/同步"行为。

---

## 3. 驱动（flow）与静态路径（static_path）

- **`flow`**（`runtime/src/flow.rs`）：`drive_link`——编译期布线验证
  （统一 `Conforms<Wire>`）后，用选定载体驱动一条 A→B 因果流；**验证在编译期，运行期零开销**
  （`drive_wired` 已删除——它是 `drive_link` 的冗余别名）。
- **`static_path`**（`runtime/src/static_path.rs`）：`run_static` / `run_declared_static`——
  把被 `Static<SUB>` 声明为"要求零成本"的子图在**编译期内联展开**（零运行时对象）。
- **声明宏**（`runtime/src/macros.rs`）：`wire!`——编译期展开的"连线 + 载体 + 验证"
  一次完成的宏/编译期技巧。

---

## 3b. 统一模型激活（runtime，`std`）

runtime 为统一模型构造子（在 `core.md` 中是**定义**；激活仍是运行/载体侧）提供**激活**：

- **存在绑定 `SlotPending<I,O>` → `SlotDrive<I, O>`**（existential binding，`runtime/src/slot.rs`）——
  对 `Slot<I,O>` 的 ∃ 存在化填充，处于**许可生命周期**（typestate，模态①）：
  `SlotPending::install`（Adding）安装编译期合规居留项（`T: PortCell<In=I,Out=O>` ⟹ core 的
  `Conforms`）、把其状态类型擦除为 `Box<dyn Any + Send>`；`commit()`（Ready→Live）授权
  `SlotDrive`——未 commit 即驱动是**类型级拒绝**（零运行期检查，落位律 A3）；`SlotDrive`
  运行期 `drive`/`swap`（swap 递增代，此前创建的 `Seat` 以陈旧引用被拒绝）；`retire()`
  终结许可（Cleaned）。这是"动态装载"的物理侧——接口固定且编译期 T1 验证、居留项
  运行期存在化。
- **`drive_seq`**（`runtime/src/flow.rs`，`no_std + alloc`）——`Rep<N,C>` 的生成/无界计数侧：
  把一组运行期 `IntoIterator` 输入依次流经同一 cell，收集输出，状态跨次保持（计数由运行期
  决定，非编译期）。
- **`drive_feedback_inline<BODY, FEED>`**（`runtime/src/flow.rs`）——`Feedback` cell 形式的
  物理激活：每个输入步一次内联无缓冲回环（`BODY -> FEED -> BODY`），由 **Moore 声明**把关
  （`FEED: Moore`，模态 ④——声明非证明）。
- **`contract` 模块**（`runtime/src/contract.rs`）——部署期与编译期接缝契约：`Moore` 标记（④）、
  `assert_capacity_nonzero`（②）、`validate_cost`/`validate_capacity`/`validate_seam`（③）、
  `ContractError`。
- **`obligation` 模块**（`runtime/src/obligation.rs`）——义务类类型系统（投递态 × 资源类 ×
  引用有效 × 生命周期）与**义务账本**（`LEDGER`）：机器可读的宪法摘录（接缝 × 义务 × 模态 ×
  见证 × 符合性测试），执行极小基律与诚实规则（A4/A5）。
- **`delivery` 模块**（`runtime/src/delivery.rs`，std）——投递四态税则：`Full`/`Closed` 自
  `mpsc` 错误机械化且被拒值随错误回传（②③）；`Timeout`/`Cancelled` **声明**为模态④
  （机械化为物理选择：定时器/请求域通道），不伪造见证。
- **`mailbox` 模块**（`runtime/src/mailbox.rs`，std）——反饥饿有界邮箱（actix 型）：容量 =
  `CAP` 缓冲槽 **+ 每生产者 1 个保底席位**；三投递模式——`try_send`（严格，满即
  `Full(v)` 值回传）、`send`（阻塞背压，占自身保底席位等待）、`fire`（尽力：缓冲槽优先，
  再占自身席位）；`recv` 阻塞且不返回 `Empty`（空态由 `try_recv` 观察）；关闭排空后投递
  得 `Closed(v)`。模态② 容量门（`CAP ≥ 1`）。`bounded_pump` 保留教学形态；邮箱是同一
  义务类（每生产者席位）的反饥饿实例。
- **`profile` 模块**（`runtime/src/profile.rs`）——**剖面目录**（六元组 C 构件；F↦C(F)）：
  `KernelProfile`（零分配预算）、`ServiceProfile`（每消息预算 + Full/Closed 机械化）、
  `ToolProfile`（外部）；剖面 = 模态① 类型令牌 + 模态③ 预算门——`assemble_profile<P,A,B,C>()`
  拒绝超预算载体，同一拓扑换剖面即换预算门、不改拓扑（T6）。载体白名单为规范文档
  （开放 `Carrier` impl 无法在类型层禁入——A5 诚实声明）。
- **`law` 模块**（`runtime/src/law.rs`，std）——运行期律探针（T 构件深化）：配对律
  （N 投递 ↔ N 判定；已收 ≤ 已投）、序列单调律、广播扇出计数律；`debug_assertions`
  门控、release 零开销（LiteOS `LOS_ASSERT` 先例）。
- **`assemble_link` / `assemble_seam`**（`runtime/src/flow.rs`）——**模态③ 的接线入口**：在
  部署装配点**一次**校验成本（有界接缝还校验容量），通过后返回 `drive_link` 函数指针
  （`Driver<A,B>`）；预算越界 = **装配失败**，绝非运行期静默成本。（`BoundedCarrier` 自带的
  编译期门是模态②；`assemble_seam` 在部署期兜底无门载体。）

两类均安全（`#![forbid(unsafe_code)]`）；`SlotDrive` 为 `std` 门控，`drive_seq` 与
`drive_feedback_inline` 非 `std` 门控——动态税局部化到缝上（见
[`unified.md`](unified.md) §5）。

## 4. 模块化与可替换（可扩展的物理载体）

- 每个 carrier 是**独立单元**，可作为单独 crate。
- 新的物理载体通过实现 `Carrier` trait 挂入，**不改 cell 拓扑**：例如用带其他调度/
  时序语义的通道载体替换队列/通道形态的载体，或用其他底层机制替换零分配载体。
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
cargo build/test --manifest-path runtime/Cargo.toml   # runtime（25 集成 + 5 契约单元测试）
cargo run --manifest-path runtime/Cargo.toml --example carrier_demo
cargo run --manifest-path runtime/Cargo.toml --example threaded_flow
cargo run --manifest-path runtime/Cargo.toml --example redis_like -- --corpus 500   # miniredis 子系统用例
cargo test --manifest-path runtime/Cargo.toml --example redis_like                 # 6 个 cell 单元测试
cargo bench --manifest-path runtime/Cargo.toml --bench carrier
```

**已达成（证据链）**：
- runtime 只依赖 cell_core（新核心），不依赖任何 v0 模块。
- 载体目录：Inline（栈上函数传·零分配）/ Queue（堆队列中转）/ Bounded（有界通道，
  编译期 `CAP ≥ 1`）/ spawned_flow（跨线程 mpsc）/ static_path / wire!（声明宏）。
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
- **零成本承诺的作用域**：承诺的是**族 B 为零**（抽象不因区分需求收费）；跨线程边的**族 A**
  （同步/唤醒/可见性）是等价手写多线程程序同样付的物理对价，不在"抽象税"之列（衔接
  `foundations.md` §8.6 第 8 条）。

---

## 9. 已知开放边界（诚实记录）

> 下列是 runtime 定位内的**薄边**，由真实用例暴露，当前**未解决但已如实记录**。它们
> 属于"工程叠加/优化 + 一处理论边界"，不改核心（`cell_core`）的既有构成。

### 9.1 背压 / 有界缓冲
无界 mpsc 形态（队列/线程输送）由 `QueueCarrier`/`spawned_flow` 覆盖（后者**无界**）；
真实系统需要"生产快而消费慢"的有界 + 背压语义。
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
- **已闭合**：一等短路载体——`ShortCircuit`/`ResultCarrier`/`MaybeCarrier`
  （`carrier.rs`）+ `drive_try_carrier` 入口：`Ok` 直通 `B`，`Err` 短路（**`B` 不执行**）；
  标准 `Carrier` 的界（`B::In = A::Out`）无法表达 X-lane，故以其一等能力形态实现，
  `Carrier` trait 不变（T6 不受影响）。失败×背压的联合语义
  已由 `bounded_pump_try`（`flow.rs`）落地：`Ok` 投入有界队列（满=阻塞背压），
  `Err` 短路（不投队列、计数）。

### 9.3 外部输入源的接入接缝（IO 事件 ↔ flow）
文档已声明"IO 是物理/载体可替换"，但"外部世界（socket 事件等）如何正式成为一条因果流
的 `in`"这一落地接口未形式化。**首案例已落地**：`redis_like`
（`runtime/examples/redis_like`，`--tcp PORT` / `--selftcp`）——纯 std TCP 服务器：
每连接有状态 `LineSplit`（跨块缓冲）→ `CmdParse`（类型化错误、短路）→ **有界通道（背压）**
→ 持有 `StoreState` 的存储工作线程（`DataStore` 全函数，无 panic 路径）→ RESP 回执路由
（每连接 FIFO 顺序、EOF 时写半关闭）。
- 落层：**runtime**（事件基座即一类载体/驱动）。
- 方向：提炼事件基座（event-substrate），使外部事件成为一类可替换的输入载体/驱动；
  `redis_like --tcp` 是该接缝的参考实现；载体类接口本身的形式化仍开放。

### 9.4 失败 × 背压同时发生
已由 `bounded_pump_try` 闭合：缓冲满与处理失败同时出现时，失败值短路（不投队列、
计数），成功值继续在满队列上阻塞（背压）——失败与背压正交，且各自显式。

---

## 10. 结论

> runtime = `cell_core` 的**物理层实现用例**：载体目录（Inline/Queue/Bounded/
> spawned_flow/static_path/wire!）+ 兑现验证，模块化可替换，解释并验证"同一张静态图可插进多种物理
> 执行（内联/队列/跨线程），每种有可验证的语义等价"。载体可通过实现 `Carrier` trait
> 挂入而不改拓扑，使物理层具备可扩展性；已闭合项（背压、失败×背压）与边界内的
> 开放问题（IO 接缝、一等短路载体）已如实记录，作为后续迭代的驱动输入。
