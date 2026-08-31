> **语言：** 中文 · [English](../en-us/semantics.md)

# axiom-semantics（原名 axiom-runtime）：语义 / 契约层

> **状态**：作为 runtime → **semantics** 重定位的第一步，在实验分支
> `rework/rename-runtime-semantics` 上改名（runtime = 语义函子 ⟦core 形状范畴⟧ → 行为范畴）。
> 本页散文完整重述——把现已不准确的"物理层"框架改为语义/契约层框架（物理/绑定实现归
> `axiom-instances`）——是后续轮，尚未撰写。机械改名（目录/包/crate/文档路径）已完成且测试全绿。
>
> **性质**（遗留框架，修订中）：axiom 的**物理层架构规范**。回答"axiom 的物理层是什么"：核心
> `cell_core` 只声明**因果数据流**（`A.out -> B.in`），runtime 回答唯一一个问题——
> **这条流的值怎么从 `A.out` 到 `B.in`，以何种时空成本**。本卷描述 runtime 的形态，
> 与已收敛的实现（分层：`semantics/src/{checks,movers,seams,drive}/*.rs`）一致。
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

载体目录（`semantics/src/movers/carrier.rs`）：

| 载体 | 物理方案 | 时空成本 | 线程 | 模块 |
|---|---|---|---|---|
| `InlineCarrier` | 栈上函数直接传（`B::step(A::step(x))`）；编译期展开（Direct 已并入） | 零分配、内联 | 单线程 | carrier.rs |
| `QueueCarrier`（std） | 堆队列中转（`Box<dyn Any>` 每消息分配） | 每消息分配 | 单线程内 | carrier.rs |
| `BoundedCarrier<CAP>`（std） | 有界通道中转（`CAP >= 1` 编译期强制） | 每消息分配 | 单线程内 | carrier.rs |
| `spawned_flow`（std） | mpsc 通道 + 独立线程，`B::State` 在专用线程；worker panic 经回执传播 | 每消息分配 + 同步 | **跨线程** | carrier.rs |

存储原语（非 Carrier；泵/邮箱之下的有界 FIFO）：`ring::BoundedRing<T, CAP>`——no_std+alloc，双计数器式（readable/writable），O(1) push/pop 且满/空为类型化判定（Full(v) 值随错误回传 / Empty，值守恒）；构造期一次预留分配、稳态每消息零分配。契约上单线程；跨线程变体待关键节选型裁定。服务 EmbeddedProfile（稳态零分配预算）。

> **有界 FIFO 消歧表（2026-08；外部审计对账）**。三个有界 FIFO 按设计并存，非冗余：
>
> | 原语 | 阻塞语义 | 生产者 | 饱和面 | 归属 |
> |---|---|---|---|---|
> | `BoundedQueue` | `push` 阻塞；`try_push` → `Full(v)` | 多（std 通道） | Block / Fail（调用侧选择） | buffer.rs（std） |
> | `BoundedMailbox` | `send` 驻留自身保底席位；`try_send`/`fire` 非阻塞 | 多，**反饥饿（每生产者一席）** | Block / Fail / 尽力（三模式） | mailbox.rs（std） |
> | `BoundedRing` | 无（仅存储）；`push` 立即 `Full(v)` | 单（单线程契约） | 尝试 push = 判定 | ring.rs（no_std+alloc） |
>
> `BoundedCarrier<CAP>` 现由 `BoundedQueue` 承载；中期把其内脏换到 `BoundedMailbox`
> （消一层包装）并为 `BoundedQueue` 开弃用轨道为开放项；上表先行回应"堆砌"观感。
> 另记（docgate 盲区）：正文散文中的 API 名无法机检——仅 ```rust 围栏被编译；已知边界。

### 载体目录六元组（S/L/T/C/V/R；2026-08）

| 载体 | S（接口与可观察行为） | L（规范强度） | T（符合性） | C（剖面） | V | R |
|---|---|---|---|---|---|---|
| `InlineCarrier` | 直通；饱和 N/A；零分配 | MUST：纯中转（T1） | 拓扑测试＋C9 基准 | Kernel/Embedded/Tool | 0.3 | 注册表（C3） |
| `QueueCarrier`（std） | 堆中转；Block（保守） | MUST：声明每消息分配 | 成本门（validate_cost） | Service/Tool | 0.3 | 注册表（C3） |
| `BoundedCarrier<CAP>` | 有界中转；CAP≥1 门；Block | MUST：容量见证（②） | assert_capacity_nonzero＋有界测试 | Kernel/Service | 0.3 | 注册表（C3） |
| `spawned_flow`（std） | 专用线程；panic 传播 | MUST：族 A Sync 声明（Z1） | 跨线程等价（T6）＋拆解测试 | Service/Tool | 0.3 | 注册表（C3） |
| `ResultCarrier`/`MaybeCarrier` | X-lane：Ok 直通、Err 短路（B 不执行） | MUST：失败为值 | 短路测试（§9.2） | Tool | 0.3 | 注册表（C3） |
| 事件基座（`ChunkSource`/`pump_events`） | 外部事件 → `A::In`；断连停止拉取 | MUST：配对律（N↔N） | 泵测试＋账本行 | Service/Tool | 0.3 | 账本（C11） |
| 异步接缝（`Poller`/`SeamPoller`） | 轮询；期限判定（同步域 TimedOut） | MUST：step 永不等（D2） | 异步接缝测试＋账本行 | Service/Tool | 0.3 | 账本（C11） |

### 第三方适配器指南（2026-08）

接入物理实现而不触碰核心：(1) 为接缝实现 `Carrier<A,B>`（S：接口与可观察行为；
L：如实声明 `cost()`/`obligation()`/`saturation()`）；(2) 以外部消费者视角测试
（examples/tests 形态；声称处做 T6 采样等价）；(3) 使用开放剖面（`Tool`/`Embedded`，
`assemble_profile`）——注册门剖面（Kernel/Service）需注册，`Registered` 密封（C3），
未注册适配器在门剖面按设计编译拒绝（白名单 = 官方目录）；(4) 异步执行器实现
`Executor` 契约（C7 三层），axiom 不随附执行器（零依赖承诺，D6）。每个入口 =
声明＋检查，绝无静默默认。

每种载体**独立可选、可替换**：换一个实现不改拓扑（T6 多物理实现）。

> **可替换性分层（宪法）**：可插拔是*分层的*，不是普适的。① **机制层——义务性开放**：
> 载体、短路形态、执行器、遥测汇、事件源、剖面皆为 trait 插座；第三方经自己的 crate 接入
> （axiom-tokio 模式）。新插座的开设条件＝出现第二实现者或真实需求（极小基律）。
> ② **政策层——半开放**：饱和策略与剖面下限按部署声明。
> ③ **词汇/宪法层——封闭**：五概念、投递/模态格、义务轴是语言本身；替换它们＝另一个框架，
> 扩充须走封闭清单程序（集体裁定）。第③层不可插拔，恰是第①层能够互操作的前提。

> **放置连续谱（衔接 `foundations.md` §8.6 第 7–8 条）**：表中"单线程 / 跨线程"**不是两个
> 模型，是同一物理放置决策谱系的两端**——同一张蓝图经放置决定每条边在谱上的位置。表内各
> 载体是谱上不同位置的物理形态：单线程载体 = "所有边同线程放置"的原生形态（族 A = 0），
> 跨线程载体如实承担族 A（并发维持对价）。零成本承诺（族 B = 0，见下）对二者同等成立。

> **载体即属性（部署期物理）**：蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>`
> 走 `InlineCarrier`/`static_path`），runtime 按声明兑现。"丢弃/阻塞/同步/异步"全是
> 物理层选择（衔接 `foundations.md` §5.8）——同一蓝图换载体即换"丢弃/阻塞/同步"行为。

---

## 3. 驱动（flow）与静态路径（static_path）

- **`flow`**（`semantics/src/drive/flow.rs`）：`drive_link`——编译期布线验证
  （统一 `Conforms<Wire>`）后，用选定载体驱动一条 A→B 因果流；**验证在编译期，运行期零开销**
  （`drive_wired` 已删除——它是 `drive_link` 的冗余别名）。
- **`static_path`**（`semantics/src/drive/static_path.rs`）：`run_static` / `run_declared_static`——
  把被 `Static<SUB>` 声明为"要求零成本"的子图在**编译期内联展开**（零运行时对象）。
- **声明宏**（`semantics/src/drive/macros.rs`）：`wire!`——编译期展开的"连线 + 载体 + 验证"
  一次完成的宏/编译期技巧。

---

## 3b. 统一模型激活（runtime，`std`）

runtime 为统一模型构造子（在 `core.md` 中是**定义**；激活仍是运行/载体侧）提供**激活**：

- **存在绑定 `SlotPending<I,O>` → `SlotDrive<I, O>`**（existential binding，`semantics/src/drive/slot.rs`）——
  对 `Slot<I,O>` 的 ∃ 存在化填充，处于**许可生命周期**（typestate，模态①）：
  `SlotPending::install`（Adding）安装编译期合规居留项（`T: PortCell<In=I,Out=O>` ⟹ core 的
  `Conforms`）、把其状态类型擦除为 `Box<dyn Any + Send>`；`commit()`（Ready→Live）授权
  `SlotDrive`——未 commit 即驱动是**类型级拒绝**（零运行期检查，落位律 A3）；`SlotDrive`
  运行期 `drive`/`swap`（swap 递增代，此前创建的 `Seat` 以陈旧引用被拒绝）；`retire()`
  终结许可（Cleaned）。这是"动态装载"的物理侧——接口固定且编译期 T1 验证、居留项
  运行期存在化。
- **`drive_seq`**（`semantics/src/drive/flow.rs`，`no_std + alloc`）——`Rep<N,C>` 的生成/无界计数侧：
  把一组运行期 `IntoIterator` 输入依次流经同一 cell，收集输出，状态跨次保持（计数由运行期
  决定，非编译期）。
- **`drive_feedback_inline<BODY, FEED>`**（`semantics/src/drive/flow.rs`）——`Feedback` cell 形式的
  物理激活：每个输入步一次内联无缓冲回环（`BODY -> FEED -> BODY`），由 **Moore 声明**把关
  （`FEED: Moore`，模态 ④——声明非证明）。
- **`contract` 模块**（`semantics/src/checks/contract.rs`）——部署期与编译期接缝契约：`Moore` 标记（④）、
  `assert_capacity_nonzero`（②）、`validate_cost`/`validate_capacity`/`validate_seam`（③）、
  `ContractError`。
- **`obligation` 模块**（`semantics/src/checks/obligation.rs`）——义务类类型系统（投递态 × 资源类 ×
  引用有效 × 生命周期）与**义务账本**（`LEDGER`）：机器可读的宪法摘录（接缝 × 义务 × 模态 ×
  见证 × 符合性测试），执行极小基律与诚实规则（A4/A5）。
- **`delivery` 模块**（`semantics/src/checks/delivery.rs`，std）——投递四态税则：`Full`/`Closed` 自
  `mpsc` 错误机械化且被拒值随错误回传（②③）；`Timeout`/`Cancelled` **声明**为模态④
  （机械化为物理选择：定时器/请求域通道），不伪造见证。
- **`mailbox` 模块**（`semantics/src/movers/mailbox.rs`，std）——反饥饿有界邮箱：容量 =
  `CAP` 缓冲槽 **+ 每生产者 1 个保底席位**；三投递模式——`try_send`（严格，满即
  `Full(v)` 值回传）、`send`（阻塞背压，占自身保底席位等待）、`fire`（尽力：缓冲槽优先，
  再占自身席位）；`recv` 阻塞且不返回 `Empty`（空态由 `try_recv` 观察）；关闭排空后投递
  得 `Closed(v)`。模态② 容量门（`CAP ≥ 1`）。`bounded_pump` 保留教学形态；邮箱是同一
  义务类（每生产者席位）的反饥饿实例。
- **`profile` 模块**（`semantics/src/checks/profile.rs`）——**剖面目录**（六元组 C 构件；F↦C(F)）：
  `KernelProfile`（零分配预算）、`ServiceProfile`（每消息预算 + Full/Closed 机械化）、
  `ToolProfile`（外部）；剖面 = 模态① 类型令牌 + 模态③ 预算门——`assemble_profile<P,A,B,C>()`
  拒绝超预算载体，同一拓扑换剖面即换预算门、不改拓扑（T6）。载体白名单为规范文档
  （开放 `Carrier` impl 无法在类型层禁入——A5 诚实声明）。

- **`law` 模块**（`semantics/src/checks/law.rs`，std）——运行期律探针（T 构件深化）：配对律
  （N 投递 ↔ N 判定；已收 ≤ 已投）、序列单调律、广播扇出计数律；`debug_assertions`
  门控、release 零开销。
- **`assemble_link` / `assemble_seam`**（`semantics/src/drive/flow.rs`）——**模态③ 的接线入口**：在
  部署装配点**一次**校验成本（有界接缝还校验容量），通过后返回 `drive_link` 函数指针
  （`Driver<A,B>`）；预算越界 = **装配失败**，绝非运行期静默成本。（`BoundedCarrier` 自带的
  编译期门是模态②；`assemble_seam` 在部署期承接无门载体的校验。）

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
cargo build/test --manifest-path semantics/Cargo.toml   # runtime（25 集成 + 5 契约单元测试）
cargo run --manifest-path semantics/Cargo.toml --example carrier_demo
cargo run --manifest-path semantics/Cargo.toml --example threaded_flow
cargo run --manifest-path semantics/Cargo.toml --example redis_like -- --corpus 500   # Redis-like 子系统用例（仓库内示例）
cargo test --manifest-path semantics/Cargo.toml --example redis_like                 # 6 个 cell 单元测试
cargo bench --manifest-path semantics/Cargo.toml --bench carrier
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

这些用例是"基于 axiom/axiom-semantics 构建真实程序"的构建用例，也是 runtime 迭代与
等价性验证的载体。旧版同类用例（含 TCP 服务器形态）可在 git 历史中恢复作参考
（`git show main:semantics/examples/<name>/main.rs`）。

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
- **cell 内禁 panic 约定（A3；2026-08）**：工程约定——cell 的 `step` 不得 panic；
  失败必须是值（`Out = Result`）；违反者为声明者责任。跨信任边界防护用
  `flow::drive_catch`（`catch_unwind`；**External 级高成本**——panic 后状态可能
  半更新，属接缝责任）；热路径不付此税。`spawned_flow`/`bounded_pump*` 已传播
  跨线程 panic（拆解显式化）。
- **拓扑级资源预算（C4 可行子集；2026-08）**：线程数可数（`spawned_flow` 每实例
  一条线程；装配期算术）；分配可由 `CarrierCost` 代数求和（链每消息类 = 各段最
  大，按声明序；`validate_cost` 已逐缝强制预算）；栈深一般不可判——编译期栈深
  推导不承诺（诚实划界，无伪推导）。机械子集锁定于 `semantics/tests/resource_budget.rs`。

---

## 9. 已知开放边界（薄边）

> 下列是 runtime 定位内的**薄边**，由真实用例暴露，当前**未解决**。它们
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

> **错误代数政策（C15-T2；2026-08 补充）**。"失败为值"（概念 1 实例）的**传播规则**
> 现成文：
> - **类型层（已强制）**：`E` 是 `Out = Result<X,E>` 的一部分；`B::In ≠ A::Out`
>   的接缝无法编译——跨段错误类型兼容由 T1 强制，非约定（netpath 的单一共享
>   `NetErr` 是类型事实，不是糖）。
> - **政策层（自由，由 driver/接缝决定，此处成文）**：
>   - *fail-fast*（首错即止）：`TryChain`/`drive_try_carrier`——E 不合并；
>   - *collect*（聚合 E）：须显式适配 cell（E 保持为值，core 内无隐式聚合）；
>   - *union/提升*（各段 E 联合）：须适配 cell；
>   - *degrade*（Err 时回退值）：`MaybeCarrier`——`None` 取代失败。
>   政策是 **driver 侧放置（L4）**，绝非 core 概念；可经有界域采样验证（C8-2
>   反例搜索），绝不可由结构单独判定（T5）。

### 9.3 外部输入源的接入接缝（IO 事件 ↔ flow）
文档已声明"IO 是物理/载体可替换"，但"外部世界（socket 事件等）如何正式成为一条因果流
的 `in`"这一落地接口未形式化。**首案例已落地**：`redis_like`
（`semantics/examples/redis_like`，`--tcp PORT` / `--selftcp`）——纯 std TCP 服务器：
每连接有状态 `LineSplit`（跨块缓冲）→ `CmdParse`（类型化错误、短路）→ **有界通道（背压）**
→ 持有 `StoreState` 的存储工作线程（`DataStore` 全函数，无 panic 路径）→ RESP 回执路由
（每连接 FIFO 顺序、EOF 时写半关闭）。
- 落层：**runtime**（事件基座即一类载体/驱动）。
- **载体类已形式化并落地**（`semantics/src/seams/event.rs`）：事件流（`EventStream`，
  条目级输入源）+ 块源适配（`ChunkSource`：`io::Read` 原始源 + 分割器 + 跨块状态，
  含通用行分割 `split_lines`）+ 泵驱动（`pump_events`：变换 cell → 投递裁决
  `PushVerdict` → 配对计数 `EventPumpStats`）。`redis_like --tcp` 是该接缝的
  参考实现，`server.rs::handle_conn` 已用本载体类驱动（行为不变，selftest 字节级一致）；
  失败也是数据（解析错误经通道转发为 `-ERR`，不被泵短路吞值）；消费端断连 ⟹ 泵
  停止拉取（拆除，`dropped` 计数，不静默延续）；块容量 N≥1 经模态②门拒绝退化态
  （boundary-ontology 命题 2.7）。账本行：`event::pump_events`（LEDGER_STD_EXTRA）。

### 9.4 失败 × 背压同时发生
已由 `bounded_pump_try` 闭合：缓冲满与处理失败同时出现时，失败值短路（不投队列、
计数），成功值继续在满队列上阻塞（背压）——失败与背压正交，且各自显式。

---

## 10. 成本语义（Z1；零成本承诺的形式化核心）

运行时成本主张的形文法——**边成本 = f(载体, 放置, 类型)**：

```text
edge_cost(seam) := class(f):
  class(Inline, static, ZST)        → ZeroAllocInline
  class(Queue|Bounded, static, …)   → PerMessageAlloc            （每消息堆）
  class(spawned_flow, static, …)    → PerMessageAlloc + Sync     （族 A）
  class(Slot 存在化, …, erase)      → PerInstallAlloc + 间接    （动态税，C9）
  class(any, …, dyn 擦除)           → + 每次驱动 downcast

组合（C4，已钉）：链成本 = 各段最大。
预算（模态③）：     逐缝 declared ≤ budget（validate_cost）；
                     剖面义务下限（C10）。
```

**族 A 不可消（陈述性证明骨架；模态④）**。断言：跨线程增量（Sync＋每消息同步）
**不可被抽象消去**——只能等价转移。骨架：(1) 因果流跨线程必然经共享内存＋同步
（唤醒/可见性）于接缝；(2) 零成本承诺是**相对等式**（foundations §0：runtime 成本
≡ 等价手写程序成本）——手写多线程程序付同税；(3) 若族 A 税可消，则存在零同步的
跨线程传值，违背流的因果序/可观测性——矛盾。骨架是陈述而非机器证明（Rice 边界，
模态④）。测量语料见证：`dynamic_tax.rs`（C9）与 `bench_common.rs` 噪声底方法；
布局敏感性如实记录，绝不以单次数字立论。

## 11. 结论

> runtime = `cell_core` 的**物理层实现用例**：载体目录（Inline/Queue/Bounded/
> spawned_flow/static_path/wire!）+ 兑现验证，模块化可替换，解释并验证"同一张静态图可插进多种物理
> 执行（内联/队列/跨线程），每种有可验证的语义等价"。载体可通过实现 `Carrier` trait
> 挂入而不改拓扑，使物理层具备可扩展性；已闭合项（背压、失败×背压）与边界内的
> 开放问题（IO 接缝、一等短路载体）作为后续迭代的驱动输入。

---

## 附录：源码布局与异步路径

源码按层分组：`checks/`（接线检查与承诺账本：contract、profile、obligation、law、
delivery）、`movers/`（值的搬运器：carrier、buffer、ring、mailbox）、`seams/`（等待、
事件、观测：async_seam、event、telemetry）、`drive/`（流通组合与驱动：flow、slot、
enum_slot、static_path、macros）。`instances/src` 下为 `backend/`（async_driver 与
tokio_exec）；`examples/sql-over-redis/src` 下为 `plans/`（sql_plan、redis_plan）。

异步路径：runtime 声明 `Executor` 契约（`seams::async_seam`）；实际异步路径在
`axiom-instances`（`backend::async_driver`）：等待挂进 tokio reactor，期限来自 tokio
定时器（`tokio::time::timeout` 包裹输入等待），等待期间新指令可经通道馈入。输出与
同步路径逐行一致（T6；综合用例核对 195/195 行）。`backend::tokio_exec` 为占位。观测是普通模块（用例侧 收集 → 汇总 → 打印），默认不接入。并发演示：单线程
服务 N 会话，墙钟与 N 无关；逐步校准（release，min-of-N + 自噪音下限）：sync
≈ 0.5µs/行，async ≈ 0.9µs/行。本主机上 tokio 计时等待量子 ≈ 15.6ms。

tokio 是异步默认后端（`tokio` 特性在 `async` 门后引入引擎）。第三方物理实现适配
（异步运行时替换层、第二后端）推迟；适配协议在第二实现者出现时定义（接缝先于
socket）。

开放项：负载下多核心并行未实测；账本行 Timeout 升 ②③ 待权威变更；真实网络异步 IO
（tokio `net`）未接——现用通道馈入。
