# 理论归档：六份早期理论文档的唯一内容合并（历史记录）

# Theory Archive: Unique Content Consolidated from Six Early Theory Documents (Historical Record)

> **性质**：I1 层历史归档（`docs/internal/theory/`）。本文件由六份早期文档合并而成；
> 六份原始文件按重组决议删除。删除前已按归档核对规则保证：本文件收录了六份原始文件
> 中**未被公开规范文档承接的全部唯一内容**；凡与公开文档重复者一律不收录（见各节
> "superseded by" 标注）。本归档不再演进；现行规范以公开文档为准。
>
> **来源文件**（均已删除，git 不跟踪原始版本）：
> 1. `axiom-theory-foundations.md`
> 2. `paradigm-notes.md`
> 3. `unified-design-proposals.md`
> 4. `compile-time-core-direction.md`
> 5. `refactor-plan-runtime-carriers.md`
> 6. `refactor-plan-compile-time-core.md`
>
> **合并规则**：
> (1) 保留范围 = 各来源文档中公开规范（`docs/en-us|zh-cn/`）未承接的内容；
> (2) 每条保留内容附承接标注 `superseded by: docs/<en-us|zh-cn>/<doc>.md §<号>`
> （标注不确定者记为"见对应公开文档"；标注仅表示结论形态的承接方，分析细节为本归档独有）；
> (3) 文体为严格论文式：无第一人称、无情绪词、无过程俗语（检查表见 `README.md`）；
> (4) 术语与公开文档一致：型位（typed hole）、布线/连接实例、三绑定态
> （型位 `Slot` 未绑/定义、`Wire` 编译期绑定、`SlotDrive` 运行期绑定）、四模态 ①②③④
> （① 结构见证 · ② 编译期常量见证 · ③ 部署期验证 · ④ 声明）。
>
> **编号说明**：来源文档引用过早期草稿编号（如 `foundations.md §15`、`Axiom 15.1`、
> `Theorem 15.1–15.4`、`Corollary 15.3a`、`§3.3`/`§5.9`）；现行公开文档编号为 §0–§8，
> 本归档一律按现行编号标注。

---

## 1. 组合系统论的推导档案

## 1. Derivations of Compositional Systems Theory

> 来源：`axiom-theory-foundations.md`。该文档的承诺、理论家族公理、统一公理集、定理
> T1–T9、数学表达、蓝图形态/静态性/runtime 定位/FlowKind 等主体内容已被
> `docs/en-us/foundations.md` §0–§7 与 `docs/en-us/core.md` §5 逐节承接，不重复收录；
> 本节只保留其未被承接的推导档案。

### 1.1 连接池的本质

> superseded by: docs/en-us/foundations.md §1.2（连接数量不改变类型平面）、§5.9（运行期修改的作用对象与边界）

1. **平面归属**。连接池位于实例平面，是宿主对"同一类连接实例"的一种资源复用策略。
   类型平面只声明一条布线与一个连接类型（静态，图中仅记录一次）；实例平面承载
   N 条连接实例 {γ₁, …, γ_N}，动态生灭。
2. **池化是工程权衡而非结构**。池化把每条连接的创建/销毁成本（网络握手、TLS、
   进程 fork）摊销为预分配槽位与复用。
3. **连接池不是底层原语，而是应用层模块**：一个遵守开放系统定义的有状态服务，
   内部状态为可复用的连接槽集合。它不触碰类型平面、不改布线，只优化实例平面的效率。
4. **概括**：连接池 = 把实例创建/销毁从每次操作摊销到批量的资源管理层；本质是
   时间-空间-复用三方权衡在实例平面的物化。

### 1.2 三个系统的统一视角

> superseded by: 抽象结论见 docs/en-us/foundations.md §5.3（布线是任意拓扑关系；多对多是常态）；
> 案例库分项分析文件已并入本归档与规范文档后删除，下表为本归档保留的对照记录

| 系统 | 开放系统（细胞） | 布线 | 实例化宿主 | 组合策略 |
|---|---|---|---|---|
| PostgreSQL | 查询（声明式 SQL：输入依赖声明，输出 = 结果集） | 隐式：查询规划器仲裁数据访问（共享缓冲/锁） | 进程 + 共享缓冲池 | 扩展打包（plugin/system 目录） |
| tokio | 任务（状态 + 消息处理经通道） | 显式：`mpsc::Sender`（N→1 指向接收队列） | 每任务一个 channel/队列 | 任务编排（join/select/作用域） |
| Redis | 订阅者/客户端（事件值语义） | 值语义：PUBLISH/SUBSCRIBE = 分发契约 | 单线程宿主（client 对象） | 模块打包 |

观察：
- PostgreSQL 的布线是隐式的——SQL 声明访问意图，规划器在运行期仲裁数据访问路径；
  物理上做"零显式连接"（共享缓冲/锁仲裁访问，而非每条访问一条显式通道）。
- tokio 把布线显式化为通道（N→1 点对点），channel 是实例化宿主。
- Redis 把布线做成事件的值语义（PUBLISH/SUBSCRIBE = 布线的分发契约）。
- 三者同属"开放系统 + 布线 + 实例 + 组合"，仅布线隐/显、宿主形态、组合打包方式
  不同；无一为线性流水线，均为关系/矩阵/图结构（多对多是常态的实证）。

### 1.3 三种组织策略

> superseded by: 载体可替换性结论见 docs/en-us/runtime.md §4；"输入经端口、实例必有宿主"
> 的公理形态见 docs/en-us/foundations.md §1.6–1.7 与 meta-foundations.md

| 系统 | 布线物理形态 | 实例化宿主 | 并发仲裁 |
|---|---|---|---|
| Linux kernel | 共享内存 + 锁（sk_buff 跨协议栈传递、inode 缓存、file_operations 回调） | 对象 + refcount（谁引用谁 = 生命周期宿主） | 细粒度锁/原子 |
| Redis | 事件循环回调（单线程消除并发） | 单线程 client 对象 | 无（单线程消除） |
| PostgreSQL | 共享内存 + 信号量（shared buffers） | 每连接独立进程（实例化到极致的形态） | 进程隔离 + 锁 |

细节：
- Linux kernel：单地址空间 + 多线程；子系统（调度/内存/VFS/网络/驱动/中断）之间靠
  共享数据结构 + 锁连接，而非消息传递。布线 = 共享内存 + 锁；实例化 = 对象 + refcount。
- Redis：单线程充当巨型事件循环，把并发问题消除于单线程内；布线 = 回调，
  实例化 = client 对象。
- PostgreSQL：多进程（每连接一个 backend）+ 共享缓冲池；布线 = 共享内存/信号量，
  实例化 = 每连接独立进程。

**pgbouncer 印证**：pgbouncer 引入连接池即为摊销"每连接 fork 一进程"的成本，印证
1.1 节的连接池本质（池化 = 摊销创建/销毁成本，属实例平面的资源管理）。

结论：三者抽象层同构，差异全部落在物理层的载体选择（布线用什么物理媒介、实例用
什么宿主、并发怎么仲裁）；这与 axiom"载体可替换、抽象层统一"的定位一致。

### 1.4 物理层全景

> superseded by: 零成本的物理含义见 docs/en-us/foundations.md §0（产物通道/观测通道双通道）与 §5.2；
> 物理栖所枚举为本归档独有

**三种存在栖所**：
- 栈：生命周期 = 一次函数调用；创建 = 压栈，销毁 = 弹栈。
- 堆：生命周期不确定；创建 = 一次 alloc，销毁 = 一次 drop/free（allocator 管理）。
- 静态区：生命周期 = 程序寿命，编译期固定地址。

执行上下文（线程/进程）= 一个"栈 + 寄存器 + IP"的调度单元；内核/用户态 = 两级特权。

**零成本的关键**：遵守 axiom 约束的实例，其分配位置/大小/生命周期与手写等价程序
一致；axiom 不产生任何记账性质的额外对象（无额外连接对象、无额外模块管理机构）。
编译后蓝图只是一段按类型正确展开的指令序列。

**布线在物理层**：
- 静态路径：布线 = 零（数据在寄存器/栈上直接传递，无连接对象）；
- 动态路径：布线 = 一个通道对象（mpsc 队列 / Arc<RwLock> / 原子槽）。

**"不断创建/销毁"的具体对象**（不是抽象的"模块"）：
- 栈帧（函数调用）→ 压栈/弹栈；
- 堆对象（Box/Vec/Arc/HashMap/sk_buff/inode/sock）→ alloc/free，refcount 决定归零销毁；
- 缓存/缓冲池（page cache / dentry cache / shared buffers / 连接槽）→ 预分配大块 + 复用
  （即"池化"在底层的形态）；
- 锁/原子（原子计数 / spinlock）→ 仲裁共享数据并发。

**全景**：底层 = 一个地址空间内的若干执行上下文，交替压栈/弹栈（函数）、alloc/free
（对象）、加锁/解锁（仲裁共享）、读写共享缓存（预配复用）。抽象层的模块/布线/图只是
这堆物理动作上的语义标签；axiom 保证这些标签不改变物理动作的成本——同样的栈帧、
同样的分配、同样的锁，一个不多一个不少。

**零成本的物理含义**：不是"抽象很高效"，而是"抽象在物理层没有额外动作"。

### 1.5 载体能力清单（七形态）

> superseded by: 已落地形态见 docs/en-us/runtime.md §2（Inline/Queue/Bounded/spawned_flow）与
> §3（static_path/wire!）；第 4–7 形态（无锁环、事件循环、进程隔离）仍未落地，属开放载体目录

axiom 载体层（runtime 物理目录）应覆盖的物理形态：

1. **栈上直传**（静态，零对象）—— `Inline`；
2. **堆队列**（mpsc/sync_channel）—— `Channel`/`BoundedBuf`（阻塞/丢弃/覆盖）；
3. **单槽 / 共享内存**（Latest/SharedState）—— 确定性、跨域；
4. **无锁环**（lock-free ring）—— SPSC 热路径；
5. **锁仲裁的共享缓冲**（原子/互斥）—— 对应 kernel/Redis 式共享数据；
6. **事件循环回调**（单线程宿主）—— 对应 Redis；
7. **进程隔离 + 共享内存/信号量**—— 对应 PostgreSQL（`Subprocess`）。

这些公共载体构成 N2N 物理形态的可替换目录，使同一抽象图可挂到不同物理世界。
共识：N2N 归物理层，axiom 只提供载体契约与兑现验证。

### 1.6 E1–E7 演化路线

> superseded by: 工程落地现状见 docs/en-us/core.md（`cell_core` 四构件）与
> docs/en-us/runtime.md（载体目录）；E5/E6 未落地（见 docs/en-us/foundations.md §7 开放问题）

| 步骤 | 公理 | 工程 |
|---|---|---|
| E1 统一端口体 | A1 | 机器/复合/共享数据共用"端口体"接口（开放系统） |
| E2 布线组合 | A2, B2 | 组合 = 任意布线图（关系）+ 端口提升；结构平面用"布线"而非固定 point 链接 |
| E3 连接实例化 | A3, I1 | 布线实例化为动态连接实例（通道）；实例挂宿主上下文；承载动态连接池 |
| E4 两平面清晰化 | A4, I1 | 实例层一等化：模块/连接实例在宿主体内动态生灭，类型平面不变 |
| E5 行为契约 | A5 | 行为接口 + 行为等价替换（seam 三角色、可替换性） |
| E6 分层验证 | A6 | 局部-全局验证粘合；视图-底层不漂移 |
| E7 载体目录 | — | runtime = 物理设计目录 + 兑现验证 |

注：本表为理论收敛期的目标设定；E1–E4 与 E7 已由公开文档与代码承接，E5/E6 为开放项。

### 1.7 meta-公理 M1（万物经端口）

> superseded by: 端口体为唯一交互面的定义见 docs/en-us/foundations.md §1.1、§5.1；
> "自由变量在结构层面不存在"的落位见 meta-foundations.md（构成三分区）

**M1**：一切不属于某个开放系统的内部状态、也不经端口交互的"自由变量"，在结构层面
不存在——它是物理层实现细节。

### 1.8 理论与计算机/物理的关系

> superseded by: docs/en-us/foundations.md §0、§5.2、§5.4、§6（边界）
> 与 §3 定理 T7（动态税下界）；原编号 Corollary 15.3a 对应现行 §6 边界论述

1. **抽象层是元语言，不参与运行**：抽象（端口、连接、图）在静态路径编译后不存在，
   物理层只见栈、地址、指令（对应公开文档的两世界分离）。
2. **零成本 = 编译期折叠**：运行时成本必须（且只能）等于手写等价程序的成本，
   差值仅来自编译器优化噪声；允许的唯一额外开销是编译时间（单态化、内联）。
3. **N2N 属于物理层**：多对多的并行调度、队列仲裁、借用、缓存、线程是 tokio 等物理实现的领域，axiom 不重造；axiom 提供载体契约与
   兑现验证（check_spec），使 N2N 实现可替换地挂入。
4. **动态税不可消除且正当**：当且仅当结构必须在运行期确定（配置/插件/动态拓扑）
   时付费；否则走静态路径。

---

## 2. 元层推理档案

## 2. Meta-Level Reasoning Archive

> 来源：`paradigm-notes.md`。该文档的结论层内容（三态定义、四模态、信任架构、分层）
> 已被 `docs/en-us/foundations.md` §1.0、§8.2–8.4、`boundary-ontology.md` §6–§7 与
> `meta-foundations.md` 承接；本节只保留未被承接的推理框架。

### 2.1 完备 vs 极小：分层法则

> superseded by: 概念层封闭判据见 docs/en-us/foundations.md §8.3（封闭判据）；
> 极小基见 theory/meta-foundations.md 定义 1.9（极小基律，与本节分层法则互补）

定义：
- **完备（complete）**——概念层性质，绝对的、逻辑/语义的：生成元集合的闭包 =
  整个可描述的概念空间；不关心所用元素的数量，关心边界是否被触及。
- **极小（minimal）**——实现层性质，相对的、物理/工程的：在给定的生成能力之内
  用最少的元素/最省的路径表达。极小是完备达成之后的二阶优化，不是概念本身。

关系：极小优化"给定能力内的路径"，完备决定"能力本身够不够"；极小不扩展生成能力。

**分层法则**：

| 层级 | 法则 | 目标 |
|---|---|---|
| 概念层 | 求「完」 | 描述力触及边界；可增、可澄清，不为减法而减法 |
| 实现层 | 求「简」 | 在概念完备的前提下，用最省、最清晰的路径实现 |

推论：减法在实现层成立（删冗余、合并重复），但不能上溯到概念层。概念层只有两问：
该生成元的语义是否不可约（纯度）？概念空间有无覆盖不到的点（完备）？都不是
"够不够少"。

风险：完备性失败会被误判为效率问题——精简到空也描述不了缺失的点；缺的不是效率，
是生成能力。

### 2.2 完备性诊断：失败的两类归因

> superseded by: 结论形态见 docs/en-us/foundations.md §8.2（无第六概念）与 §8.3（封闭判据）；
> 完整诊断框架为本归档独有

当生成元集合遇到完全无法描述的场景时，先诊断，不急于增删。完备性失败只有两类归因：

| 诊断 | 特征 | 归因 | 修复路径 |
|---|---|---|---|
| ① 局部缺失 | 概念空间某点落在生成元闭包之外，但新生成元可无冲突嵌入既有定义 | 概念/代码层缺失（闭包未覆盖） | 在既有公理体系内补一个生成元（加法） |
| ② 基础冲突 | 新生成元与既有定义/体系冲突，无法在既有公理体系内嵌入 | 基本定义的问题（公理/体系自身） | 回溯基本定义——重审公理 |

**判据**：新元素能否在既有定义体系内无冲突嵌入？
- 能 → ①，补生成元即可；
- 否 → ②，不是"加一个元素"能解决的，是基本定义需要修订——数学问题/分析学思路。

要点：
- ① 的修复在生成元层（闭包扩张）；② 的修复在基本定义层（公理回溯）。
- ② 不能靠删除冲突的新元素解决——删除只是回避冲突，完备性缺失仍在；必须回到底层
  定义，追问既有公理本身是否构成完备的障碍。与数学史同构：发现某性质与现有公理
  矛盾时，选择是修公理（平行公理之争 → 非欧几何），而非无视反例。
- 两条路径都独立于"减法"（§2.1：极小不等于完备；减法只适用于实现层）。

### 2.3 组合子的定义依据（组合封闭性命名）

> superseded by: docs/en-us/foundations.md §8.1（五个不可约构造概念）与
> docs/en-us/core.md §6c（构造子 → 概念实例矩阵）

- 组合子命名源自 SKI 组合子逻辑：**本身是系统，组合后仍是系统——组合封闭性**。
- 该性质保证复杂系统 = 简单子系统的组合，组合结果仍是同类系统，每层可独立验证。
- 定义（修正版）：组合子 = 完备描述的基本单元。完备是第一性质，极小只是完备达成后
  可选的优化追求，不构成定义成分。

### 2.4 证明预算四原则

> superseded by: 模态体系见 theory/meta-foundations.md 定义 1.3、定理 3.1 与
> docs/en-us/foundations.md §9.5–9.6（可判定性阶梯/分层律）；"最早期时刻原则"与
> "total 性偏好"为本归档独有的设计准则，未被其他文档承接

回答"该性质该不该交给编译期证明"这一判断：证明是昂贵的断言，受可判定性、判定成本、
判定价值三重约束。

1. **证明采用判据**：采用编译期证明当且仅当 可判定 ∧ 成本可接受 ∧ 价值为正。
2. **预算范围**：封闭概念边界内的类型平面/const 平面性质走模态①②（零成本承诺的
   来源）；部署策略性质走③并显式标注；语义性质只允许④。
3. **最早期时刻原则**：性质的验证时机 = 该性质最早可判定的时刻。
4. **total 性偏好**：构造子类型应当 total（任何参数化都是合法蓝图）；约束优先表达
   为可选见证或独立类型，避免带隐式前置条件的 partial 类型——悬置态（既非合法亦非
   非法）的代价高于明确的二值判定。

### 2.5 六类信任回退诊断清单

> superseded by: 总判据见 theory/meta-foundations.md 定义 1.5（诚实规则：声明不得伪装为证明）
> 与 boundary-ontology.md §5（三归宿）；六类清单为本归档独有

一切"声明 vs 强制"问题的共同根：把信任从运行时/用户移交到编译器；每一个
"声明而非强制"之处都是一次信任回退。六类典型的回退形态：

1. **空 marker**：无证明义务的 trait，空 impl 即绕过——应属模态④却伪装为①。
2. **命名盗用**：已被数学语义占据的词（如 Kleene 星）指代不同语义——读者预期被误导。
3. **见证只护单路径**：良构性外挂到某个仪式方法，绕路即静默通过——半模态的风险
   高于单模态。
4. **可编译期判定的错误推迟到运行期**——验证时机的退行。
5. **内容层能力漏进形状层**（如扇出要求值可拷贝）——层污染。
6. **证据链自污染**：未优化的测量混入验收命令——假警报训练读者忽略所有警报。

### 2.6 三层边界：证明 → 框架 → 契约

> superseded by: docs/en-us/foundations.md §8.4（分层：构造概念 vs 性质公理 vs 运行策略）
> 与 docs/en-us/runtime.md §1/§8 承接结论形态；衰减谱表述为本归档独有

"信息传递是物理层自己的事情，无法在 core 内定义"——精确表述是三层衰减，不是二分：

1. **core（证明层）**：类型系统强制形状（可判定），无契约。
2. **runtime 框架（容纳层）**：trait 接口容纳一切传递方式，提供部署期校验
   （模态③，半契约）；具体传递方式可替换、可不用，axiom 提供参考实现而非唯一/强制实现。
3. **开发者（契约层）**：具体载体的传递语义（不丢消息/FIFO/背压/线程安全）
   不可判定、无法约束，是模态④的纸面契约。

边界是衰减谱而非一刀切："无法在 core 内定义"不等于"axiom 什么也不管"——runtime
提供框架与部署期校验，只是不提供"证明具体传递语义"。纸面契约不是 axiom 的失败，
而是 axiom 对类型系统本性边界的尊重。

---

## 3. 统一模型：路线与落地审计

## 3. Unified Model: Route and Landing Audit

> 来源：`unified-design-proposals.md`。原文自述"提案已全部落地，降级为推理记录"；
> 规范表述见 `docs/en-us|zh-cn/unified.md`，构造子见 `core.md §6b`，激活侧见
> `runtime.md §3b`。本节保留路线、封顶决策、落地审计与溯源。

### 3.1 路线表 S1–S4 与封顶决策

> superseded by: docs/en-us/unified.md §3（三种代换形式）、§4（schema 表达力阶梯）、
> §5（精确动态税）；步骤表与封顶决策本身为本归档独有

| 步骤 | 内容 | 验收 |
|---|---|---|
| S1 | 在 `cell_core` 加 `Choice`/`Opt`/`Star`/`Repeat`（Kleene 层） | 编译、T1 对每次重复合法、测试 |
| S2 | 加 `Slot<I,O>` 型位（密封接口 + 参数化 T1） | 编译期 `∀T` 验证、运行期代换、测试 |
| S3 | （可选）代数层互递归 schema | 编译、测试 |
| S4 | runtime 配合：型位居留项的物理载体（装载 = 物理，T9） | 跨载体语义等价验收 |

**封顶决策**：compile-time 可证明的 schema 类封顶在**正则/代数**；一般动态图
（任意共享/环/拓扑改写）不可判定，归物理/验证边界（公开文档 T9 显式例外），
不做编译期可证承诺。

### 3.2 落地审计（P1–P3 / A–C / R–K–T 裁定）

> superseded by: 成果形态见 docs/en-us/unified.md §2.3、§6，docs/en-us/core.md §6b，
> docs/en-us/runtime.md §3b/§9；本审计（测试计数与裁定记录）是唯一保留的执行证据

- **P1（③ 有界星）**：core `Rep<N,C>`（正则/星；`RepState` 手动 `Default`、`N=0` 恒等、
  编译期 T1 验证；4 测试通过）。
- **P2（② 定义侧）**：core `Slot<I,O>` + `Conforms`/`assert_conforms`（编译期参数化
  T1："未来任何 `In=I,Out=O` 的居留项合规"；1 测试通过）。
- **P3（激活侧，runtime/std）**：`SlotDrive<I,O>`（∃ 存在化填充：install/swap/drive，
  类型擦除为 `Box<dyn Any + Send>`）+ `drive_seq`（无界计数序列驱动）；3 测试通过。
- **A（核心正则算子补全）**：一等纯 `PortCell` 的 `Choice<A,B>`（输入标号并，纯确定）
  + `Opt<C>`（可选，`Option` 变换），prelude 导出，3 测试通过；闭合正则算子 `|`、`?`。
- **B（runtime 错误/短路通路）**：`drive_try<A,B,X,E>`（`Out=Result` 约定 + 短路，
  no_std 安全），1 测试通过；设计改进（D）：新增 `TryChain<A,B>`（两个会失败的 cell
  的单层 `Result` 短链 PortCell）——psql 以 `TryChain<TryChain<Lexer,Parser>,Executor>`
  表达整条 REPL，三层错误合一短路。
- **B（有界/背压）**：`BoundedQueue<T,CAP>`（buffer.rs，std，基于 sync_channel；
  `push` 阻塞 = 背压、`try_push` 满返回 `Err` = 容量信号），2 测试通过。
- **R（有界/背压载体）**：`BoundedCarrier<CAP>`（有界通道形态的 `Carrier`）+
  `bounded_pump<A,B,It,CAP>`（真实阻塞背压：生产端满时阻塞、消费者线程 drain；
  返回输出序列），backpressure 2 测试通过。
- **C（psql/redis_like 健壮性）**：psql 改造为可失败的 REPL——`Lexer`/`Parser` 的
  `Out = Result<_, PErr>`（词法/语法错显露而非静默吞掉），`Executor` 报执行错
  （表不存在）；主流程用 `drive_try` 对 Lexer→Parser 短路（语法错不流入 Executor）；
  并修复 `SELECT *` 未识别的 bug；同时清掉 mmo/redis_like 的既有 `let_unit_value`
  警告 → runtime 全部目标（lib+examples+benches）clippy 零警告。redis_like 加
  `Config` 资源边界（max_keys/max_value，超限拒绝）、`Cmd::Protocol`（缺参/非法值
  不再静默成 0/空 → RESP `-ERR`）、`Reply::Err` 编码；4 例（psql/redis_like/mmo/netpath）
  clippy 零警告并运行通过。
- **全程约束**：core 零依赖、no_std、`#![forbid(unsafe_code)]`；core 15 测试 +
  runtime 10 测试全部通过，lib clippy 干净。

**裁定（已收束）**：
- **R（有界/背压）**：`BoundedQueue` + `BoundedCarrier` + `bounded_pump` +
  可失败 `bounded_pump_try`（失败 × 背压联合语义）。
- **K（代数/递归 schema）**：无需新核心组合子——递归/互递归图样由用户递归
  `PortCell` + 既有组合子（`Rep`/`Chain`/`Choice`/`Opt`）表达（测试
  `recursive_cell_type_composes_with_t1`）；无界生成性展开归 ∃/物理侧
  （`drive_seq`/有界泵）。
- **T（失败/全函数）**：不公理化——`step` 保持全函数，"失败"是 `Out=Result` 的值；
  穿过组合由 `TryChain`/`drive_try`（短路）承担。

### 3.3 审计修复清单 S1–S14（goal a27ebec4，runtime 代码）

> superseded by: 相关成品见 docs/en-us/runtime.md §9.1/§9.2；S1–S14 清单与 goal id
> 为本归档独有（git 不跟踪原始文档，无其他副本）

- S1：`spawned_flow` worker panic 经 catch_unwind → 回执通道 → 调用方 resume_unwind
  传播（终止性修复，测试覆盖）。
- S2：`BoundedCarrier` flow 内 `const { assert_capacity_nonzero::<CAP>() }` 编译期门
  （拒绝 CAP=0 死锁态）。
- S3：`BoundedQueue::push` 断连返回 `Err(值)`（不静默丢弃）；`pop`/`try_pop` 用
  `Result` 区分空/断连；`spare` → `capacity`。
- S4：删除死类型 `ChannelCarrier`。
- S5：删除 `drive_wired` 假 LINK 见证（≡ `drive_link`）。
- S6：删除 `DirectCarrier` ≡ `InlineCarrier` 副本（static_path/lib/示例同步）。
- S7：门禁落地：新增 `drive_feedback_inline`（要求 `FEED: Moore`）+ 测试。
- S8：去掉 `drive_seq` 多余 std 门控（no_std+alloc 可用）。
- S10：runtime Cargo 依赖 `axiom` `default-features=false`、`std=["axiom/std"]`
  （no_std 组合收束）。
- S12：`CarrierCost` 默认改 `External`（保守，防未声明自诩零分配）。
- S14：`TryChain` 措辞修正。
- 未动项：core 的 `Rep` 互 From 双界（C1）与 `Feedback` 单元双拍（C2），另行裁定。

### 3.4 溯源

- 统一模型的规范性表述：`docs/zh-cn/unified.md`、`docs/en-us/unified.md`。
- 动态税（当时编号 §3.3）、型位/墙（当时编号 §5.9）、开放问题（§7）——
  现行编号以 `docs/en-us/foundations.md` §0–§8 为准（见文件头编号说明）。
- 本提案服务于：把 axiom 从"静态蓝图实现"推向"统一设计实现"；落地后已同步更新
  正式文档。

---

## 4. 编译期核心：原则重审决策记录

## 4. Compile-Time Core: Principle Re-examination Record

> 来源：`compile-time-core-direction.md`。背景转向：axiom 核心层应围绕 Rust 编译器
> 能力（宏、过程宏、类型系统、const 泛型）设计；核心层能力到编译时被计算后为止，
> 用于提供分析、验证。决策的执行结果（cell_core 四构件、编译期验证、蓝图即类型）
> 已由 `docs/en-us/core.md` 承接；本节的取舍理由为决策记录，归档保留。

### 4.1 保留原则（在新方向下保持）

| 原则 | 理由 |
|---|---|
| 零成本承诺 | "编译期后为止"直接保证编译后无 axiom 对象；等价于手写代码自动成立 |
| trait 类型级证明 | 非法组合在类型层编译失败，是现有能力最强的机制之一 |
| no_std 目标支持 | 纯编译期核心与目标平台无关（编译器在 host 运行） |

### 4.2 推翻 / 重定义原则

| 原则 | 旧义 | 新方向下的处置 |
|---|---|---|
| 字面零依赖 | 核心零依赖 | **重定义**："编译后零运行时依赖"，编译期依赖（syn/quote 等宏依赖）允许——过程宏生态依赖它们 |
| FanOutViaTee | 拒绝隐式扇出 | **推翻**：宏可生成 N 路扇出，无需 Tee 树；多对多连接作为一等（衔接统一模型） |
| 运行时 validate_deep | 对运行时值验证，返回 `Result` | **升级**：违规改为函数宏 `compile_error!` / 类型编译失败（violation = 编译错误，非运行时检查） |
| DynamicTopology / 值形态 | 蓝图 = 运行时值 | **被挑战**：类型级图 + const 边表成为候选形态 |

### 4.3 待协商原则

| 原则 | 新方向的张力 |
|---|---|
| `forbid(unsafe_code)` | 编译期层无运行时，影响变小；保留代价低，但宏生成代码的 unsafe 纪律需另行约定 |
| 静态/动态分叉 | 保留精神，重述为"编译期可验证（静态）vs 编译期不可（留普通代码）" |
| no_std 的"零依赖"叙事 | 若核心分化出 proc-macro crate，其依赖与运行时库依赖需分开声明 |

注意：4.2 中"被挑战"一栏对 DynamicTopology 的处理其后以"蓝图即类型、无 JSON/值形态
中间层"定稿（docs/en-us/foundations.md §5.5、docs/en-us/core.md §3）。

---

## 5. 重构执行审计

## 5. Refactor Execution Audits

> 来源：`refactor-plan-runtime-carriers.md`、`refactor-plan-compile-time-core.md`。
> 两份文档的操作对象与成品已分别由 `docs/en-us/runtime.md`、`docs/en-us/core.md`
> 承接；本节的提交级执行日志、遗留清单与收敛策略为归档独有。

### 5.1 runtime 重建：七步执行序列

1. 重建 runtime/Cargo.toml：依赖新核心（cell_core），移除旧 dev；旧 runtime 源码
   移入 `runtime/_legacy_v0/` 搁置。
2. 建 `Carrier` trait + `InlineCarrier`：栈上直接传（物理 = 载体），零分配内联。
3. 建 `QueueCarrier`：堆队列跨线程传输（不同时空成本 = 不同载体）。
4. 建 `DirectCall`/驱动：编译期展开链 + 一个最小 runtime 驱动（给定蓝图按载体驱动）。
5. 示例：用 cell_core + 载体跑一个"链/广播"二阶拓扑。
6. 测试 + no_std：carrier 单测、编译期验证、no_std 构建。
7. 文档 + 收束：lib.rs 文档化定位、更新执行计划记录。

验收标准：runtime 只依赖新核心；`Carrier` 换实现不改拓扑（多物理实现成立）；
`InlineCarrier` 零分配/内联可证（编译后与手写等价）；`QueueCarrier` 支持跨线程；
每个载体独立、可单独引用；cargo build/test/no_std 通过。

### 5.2 runtime 重建执行日志 R1–R7

- **R1**（`d77d909`）：runtime 骨架：`Carrier` trait + `InlineCarrier` +
  `QueueCarrier`(std) + `DirectCarrier` + flow 驱动；旧 runtime 移入 `_legacy_v0`。
- **R2**（`2e0ce35`）：carrier_demo 用例：同一张 cell_core 蓝图多载体可替换、
  语义等价、时空成本不同。
- **R3**（`9f06463`）：static_path：`Static` 声明 → 编译期内联展开（零运行时对象）。
- **R4**（`d3a3e2d`）：`ChannelCarrier`/`spawned_flow`：真实跨线程通道载体
  （mpsc + 独立线程）。
- **R5**（`efb3404`）：lib.rs 定位文档补全（载体目录/驱动/静态路径/第三方模板）。
- **R6**（`24de0ce`）：`wire!` 声明宏：编译期展开的连线 + 载体 + 验证一次完成。
- **R7**（—）：本记录；目标达成评估。

验收结果：runtime 只依赖新核心；载体目录 = Inline（栈上函数传·零分配）/ Queue
（堆队列中转）/ Channel（跨线程 mpsc）/ Direct + static_path（编译期展开）/ wire!
（声明宏）；模块化可替换（换载体不改拓扑，定理 T6）；runtime 7 测试 + core 9 测试 +
no_std 构建全部通过。旧 runtime 源码在 `runtime/_legacy_v0/` 保留作物理思路参考。

> superseded by: docs/en-us/runtime.md §1–§6；提交哈希与 `_legacy_v0` 策略为本归档独有

### 5.3 编译期核心重构执行日志 R1–R7

- **R1**（`2d0a6c7`）：cell_core 四构件主轴线（PortCell/Link/CellChain/Static/drive）*
- **R2**（`9d01e91`）：cell_core 复杂拓扑：Feedback（环）+ Broadcast（多对多）。
- **R3**（`269d08c`）：FlowKind 移出标记（DEPRECATED）+ Blueprint 即类型（零大小）。
- **R4**（`469fd3d`）：删除死公共 API：PortRegistry/PortEntry/is_unknown。
- **R5**（`1fd8432`）：编译期布线验证：DoesWire/assert_wiring†。
- **R6**（`46b3dc2`）：示例 cell_demo：四构件作为普通 Rust 程序运行（零运行时对象）。
- **R7**（`b08ddf1`）：定位：cell_core 确立为 crate 主主轴，旧核心标 legacy。

达成记录：四构件完整、可编译、有测试（8 个）；复杂拓扑（环/广播）在类型层表达，
无 Box<dyn>/JSON/线程/FlowKind；蓝图即类型，`size_of<Blueprint<T>>() == 0`；
验证在编译期（DoesWire 类型判定，非法布线编译失败）；编译后等价手写普通 Rust
（cell_demo 实证）。

> \* R1 时期的 `Link`/`CellChain` 命名其后按命名收敛定稿为 `Wire`/`Chain`
> （见归档 §3.2 出处文档的定稿记录）。
> † R5 时期的 `DoesWire` 其后统一为 `Conforms<EXPECT>`（`assert_wiring` 经
> `assert_conforms`），`DoesWire` 全库移除。
> superseded by: docs/en-us/core.md §2–§6；提交哈希与命名演进注为本归档独有

### 5.4 遗留缓行清单（编译期核心重构）

- **FlowKind 接口层实际剥离**：`HasPortInfo::flow_kind()` 及 builtin 经 prelude 依赖
  ——完全移除需 redesign 旧端口接口 + 重构 builtin（57+ 处）；新核心 cell_core
  已不依赖它。
- **值形态/JSON（blueprint.rs）**：serialize 集成测试依赖，删除会破坏其编译；
  留待新核心确立后再处置。
- **旧模块映射**：machine/static_exec/composite/portset 等逐个映射进四构件或删除——
  需在 cell_core 补齐更多组合子（fan-in/任意图形）后逐模块迁移。

### 5.5 受控收敛策略

> superseded by: 收敛结果见 docs/en-us/core.md §8 与 docs/en-us/runtime.md §9（开放边界）；
> 策略陈述为本归档独有

方向性重构采用受控收敛策略：先立可编译的新主轴并逐项实证（类型化/零对象/编译期
验证/等价运行），旧语义以 DEPRECATED/legacy 标记呈现新方向，然后在风险可控时
再逐模块迁移/删除。不要求一步完成；旧模块删除前标注迁移/弃用状态，凡未被新核心
替代者宁可保留、不破坏编译。该策略在 8 轮内达成目标核心的能力收束，遗留为需要
独立设计的深化项。

---

## 附：来源文件处置与承接总览

## Appendix: Disposition and Supersession Summary

| 来源文件 | 唯一内容去向（本归档章节） | 重复内容承接方 |
|---|---|---|
| axiom-theory-foundations.md | 第 1 节 | docs/en-us|zh-cn/foundations.md §0–§7；core.md §5 |
| paradigm-notes.md | 第 2 节 | foundations.md §1.0（三态）、§5.8、§8.2–8.4；boundary-ontology.md §6–§7；meta-foundations.md 定义 1.5 |
| unified-design-proposals.md | 第 3 节 | unified.md §2.3/§6；core.md §6b；runtime.md §3b/§9 |
| compile-time-core-direction.md | 第 4 节 | core.md §1–§6、§8；foundations.md §5.3/§5.5 |
| refactor-plan-runtime-carriers.md | 第 5 节 | runtime.md §1–§6 |
| refactor-plan-compile-time-core.md | 第 5 节 | core.md §2–§6 |