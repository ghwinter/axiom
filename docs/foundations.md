# axiom 代数基础

> **版本**: v3 · **日期**: 2026-07-29
>
> 本文档从范畴论、类型论和系统理论的角度，对 axiom 的计算模型进行形式化定义和推导。
>
> 所有定义、公理、定理和推论均对应于 axiom crate 中具体的 Rust 类型和 trait 实现，
> 使得数学证明与代码之间的映射关系是可检验的。
>
> **结构：** 每一节以公理（不证自明的基本假设）开始，然后定义领域概念，随后推导定理和推论。箭头 $P \to Q$ 表示 $P$ 是 $Q$ 的证明前提。

---

## 目录

0. [物理基底](#0-物理基底)
1. [计算原语](#1-计算原语)
2. [端口与连接](#2-端口与连接)
3. [执行序列与调度](#3-执行序列与调度)
4. [资源代数](#4-资源代数)
5. [组合与范畴结构](#5-组合与范畴结构)
6. [部署代数](#6-部署代数)
7. [系统整体定理](#7-系统整体定理)
7.5. [工程修补](#75-工程修补数学模型与实现之间的缝隙)
8. [Rust 映射](#8-rust-映射)
9. [Curry-Howard 对应](#9-curry-howard-对应)
10. [与 V8 对比](#10-与-v8-对比)
11. [会话类型](#11-会话类型)
12. [混合系统](#12-混合系统)
13. [生命周期类型状态](#13-生命周期类型状态)
14. [统一性评估：研究脉络的归约与覆盖](#14-统一性评估研究脉络的归约与覆盖)
15. [零成本抽象：抽象层与物理层的解耦](#15-零成本抽象抽象层与物理层的解耦)

---

## 0. 物理基底

**公理 0.1（内存位置集的存在性）**  
存在可寻址内存位置集 $L$。每个位置 $l \in L$ 在时刻 $t$ 持有值 $v \in V$。记 $mem_t: L \to V$ 为 $t$ 时刻的内存状态。

**公理 0.2（计算步的存在性）**  
存在一个计算步 $(r, w, \phi)$ 的三元组操作，其中 $r \subseteq L$ 是读取集，$w \subseteq L$ 是写入集，$\phi: V^{|r|} \to V^{|w|}$ 是转移函数。

**定义 0.1（线程）**  
一个线程 $T$ 是一个计算步的序列。线程在物理层等价于一个栈——每一步压入帧，执行，弹出。

**定义 0.2（进程）**  
一个进程 $P = \{T_1, ..., T_n\}$ 是共享同一地址空间 $L_P \subseteq L$ 的一组线程。

---

## 1. 计算原语

### 1.1 纯函数

**定义 1.1（纯函数）**  
一个纯函数定义为 $f = (I, O, \hat{f})$，其中 $\hat{f}: I \to O$ 是映射函数。  
**物理实现：** 一个计算步 $(r_f, w_f, \phi_f)$，$w_f$ 仅限于当前栈帧。

**公理 1.1（栈帧隔离性）**  
一个栈帧的写入集与所有其他栈帧的写入集不相交。

> **定理 1.1（纯函数物理隔离）**  
> $\text{公理 1.1} \Rightarrow$ 对于任意 $f$，$w_f \cap L_{other} = \emptyset$。

> **推论 1.1a（可并行性）**  
> $\text{定理 1.1} \Rightarrow$ 任意 $\{f_i\}$ 可在任意 $n$ 个线程上并行执行且结果等价。

### 1.2 机器

**定义 1.2（机器）**  
一个机器 $M$ 定义为 $M = (S, I, O, \delta, \rho)$——这是 IO-Object $(S, I, O, \delta)$ 加上清理函数 $\rho$：
- 没有独立的 $Obs$ 分量。观察数据是 $O$ 中通过 $FlowKind::Observe$ 端口输出的子集。
- 没有独立的 $C$ 分量。控制数据是 $I$ 中通过 $FlowKind::Control$ 端口输入的子集。
- $S$ 分配在堆上（$L_S \subset L_P$），$\delta$ 每次调用执行一个计算步。
- $S$ 是状态空间
- $\delta: S \times I \to S \times O$ 是转移函数（Mealy 机）
- $\rho: S \to S$ 是清理函数

**物理实现：** $S$ 分配在堆上（$L_S \subset L_P$），$\delta$ 每次调用执行一个计算步。

> **定理 1.2（状态局部性）**  
> $\text{定义 1.2} \Rightarrow$ 对于任意 $M$，所有 $\delta$ 的写入集包含在 $L_S \cup w_{\delta}$ 中。

**定义 1.2a（Mealy 语义与 Moore 语义）**  
axiom 的默认 Machine 是 **Mealy 机**：输出同时依赖当前状态和当前输入，$\delta: S \times I \to S \times O$。

某些计算原语需要 **Moore 语义**：输出仅依赖当前状态，与当前输入无关。形式化为 $\lambda: S \to O$，状态转移 $\delta_S: S \times I \to S$。

**Moore 型 Machine 的构造**：将 $\delta$ 实现为"先更新状态，再从旧状态产出"：
$$\delta(s, i) = (s', \lambda(s)) \quad \text{其中} \quad s' = \delta_S(s, i)$$

即输出 $\lambda(s)$ 来自**转移前**的状态 $s$，而非转移后的 $s'$。这实现了"延迟一拍"语义。

> **定理 1.2a（Moore 延迟打破反馈环）**  
> $\text{定义 1.2a} \Rightarrow$ 在反馈拓扑 $M_1 \to M_2 \to M_1$ 中，若 $M_2$ 为 Moore 型，则 $M_2$ 的输出滞后输入一拍，打破同一时钟内的代数环。  
> *证明：$M_1$ 在时刻 $t$ 的输出依赖 $M_2$ 在时刻 $t$ 的输出，但 $M_2$ 在时刻 $t$ 的输出来自 $t-1$ 的状态，不依赖 $M_1$ 在 $t$ 的输出。*

> **工程修补 1.2a（Moore 型的首次输出）**  
> Moore 型 Machine 在首次调用 $\delta$ 时，状态 $s_0$ 是初始值。若 $\lambda(s_0)$ 无意义（如 `Option::None`），需约定首次输出为 `Idle` 而非 `Yield`。这是图灵机模型不涉及但工程实现必须处理的边界条件。

### 1.3 实体

**定义 1.3（实体）**  
一个实体 $E$ 定义为 $E = (S, name)$。实体只有状态和名字，没有输入、没有输出、没有转移函数。它是"存在"的最小声明。

> **定理 1.3（实体的可观测性）**  
> $\text{定义 1.3} \Rightarrow$ 实体 $E$ 的状态 $S$ 可以被外部观测（只需读取 $L_S$ 地址），但不参与任何计算拓扑。

---

## 2. 端口与连接

**公理 2.1（通信只能通过共享地址）**  
两个线程间没有数据行（data race）的通信只能通过共享内存地址（$L_1 \cap L_2 \neq \emptyset$）或复制值。

**定义 2.1（端口）**  
端口 $p = (T, d, f)$ 由类型 $T$、方向 $d \in \{in, out\}$ 和流语义 $f \in \{data, control, observe\}$ 构成。

**定义 2.2（接口）**  
一个接口 $\Gamma$ 是端口的有限**集**。即 $\forall p_1, p_2 \in \Gamma: name(p_1) \neq name(p_2) \lor p_1 = p_2$。

> **公理 2.2（接口的编译期静态声明）**  
> 一个 Machine 的接口 $\Gamma_{in}$ 和 $\Gamma_{out}$ 在编译期固定，运行时不可变。  
> *Rust 映射：`type Input: HasPortInfo`（enum，每端口一个 variant）+ `type Ports: PortSet`（连接类型空间与值空间）。*

**定义 2.3（连接）**  
连接 $\ell = (p_s, p_t)$ 要求 $dir(p_s) = out$、$dir(p_t) = in$、$T_{p_s} = T_{p_t}$、$f_{p_s} = f_{p_t}$。

> **定理 2.1（类型可靠性）**  
> $\text{定义 2.3} \Rightarrow$ 类型匹配的连接在语义上有效。  
> *Rust 映射：编译器通过 TypeId 检查保证。*

> **定理 2.2（观测隔离性）**  
> $\text{定义 2.1} \Rightarrow$ 观测流 $(f = observe)$ 的输出不参与任何 Machine 的 $\delta$ 输入。  
> *证明：$\delta$ 签名 $S \times I \to S \times O$ 中 $Obs$ 不存在于输入中。FlowKind::Observe 是端口标注，不是计算分量。*

> **定理 2.3（类型-值一致性）**  
> $\text{公理 2.2} \land \text{定义 2.2} \Rightarrow$ `type Input` 的 enum variant 集与 `port_schema()` 的 PortDecl 集一一对应。  
> *证明：`type Ports: PortSet` 的 `port_schema()` 由 `PortSet` 实现生成，其声明与 `type Input`/`type Output` 的 enum variant 声明同源（`declare_ports!` 宏或手动 PortSet impl 保证）。*  
> *Rust 映射：`PortSet` trait 连接 `type Input: HasPortInfo`（类型空间）与 `PortSchema`（值空间），`port_schema()` 自动派生。*

> **定理 2.4（多端口扇出存在性）**  
> $\text{定义 2.2} \Rightarrow$ 一个 Machine 可以在单次 $\delta$ 调用中向多个输出端口产出。  
> *Rust 映射：多端口扇出由 `MultiOutput::YieldMulti(Vec<O>)` 表达——输出数量在运行时确定（fan-out 机器）；固定数量多端口用 `TupleOutput::Yield(O, O)`。*

**定义 2.4（连接图）**  
系统 $\Sigma = (M_\Sigma, L_\Sigma)$，$L_\Sigma \subseteq \bigcup_{M \in M_\Sigma} Out_M \times \bigcup_{M \in M_\Sigma} In_M$。

> **定理 2.5（输出可达性）**  
> $\text{定义 2.4} \Rightarrow$ Machine $M$ 的输出端口 $p$ 的数据可达 $\iff$ $\exists \ell \in L_\Sigma: \ell = (p, \_)$。  
> *Rust 映射：连接存在性由部署层拓扑决定（`DeploySpec` + `validate_deep` 校验）；Machine 不查询输出可达性——观测短路是 runtime 的职责（`Observe` 流经链接物化）。*

---

## 3. 执行序列与调度

**定义 3.1（执行序列）**  
机器 $M$ 的 $\delta$ 应用序列：$s_0 \xrightarrow{i_1} (s_1, o_1) \xrightarrow{i_2} (s_2, o_2) ...$

**定义 3.2（调度器）**  
调度器 $\Pi: M_\Sigma \times \mathbb{N} \to \{T_1, ..., T_n\}$ 将每次 $\delta$ 调用映射到物理线程。

> **定理 3.1（函数执行等价性）**  
> $\text{定理 1.1a} \Rightarrow$ 对于纯函数集合，任意调度器 $\Pi$ 产生相同结果。

**公理 3.1（顺序约束）**  
机器 $M$ 的连续 $\delta$ 调用必须在同一线程上执行，否则 $S$ 上的竞态条件导致结果未定义。

> **定理 3.2（调度器必须遵守顺序约束）**  
> $\text{公理 3.1} \land \text{定义 3.2} \Rightarrow$ 调度器 $\Pi$ 对同一 Machine 的两次调用必须映射到同一线程。

**公理 3.2（执行原语分类完备性）**  
所有物理执行模式可分类为：零调度开销（Inline）、协作调度（Async）、抢占调度（CpuBound/CpuBoundN/ThreadPool）、进程隔离（Subprocess）。

| 原语 | 物理对应 | 隔离级别 |
|------|---------|---------|
| Inline | 同线程栈帧调用 | 共享(0) |
| Async | 事件驱动线程池 | 共享(1) |
| CpuBound | 独占 OS 线程 | 独占(2) |
| CpuBoundN(n) | N 个独占线程 | 独占(3) |
| ThreadPool | 私有有界线程池 | 独占(3) |
| Subprocess | 独立进程（IPC） | 隔离(4) |

> **推论 3.2a（执行原语完备性）**  
> $\text{公理 3.2} \Rightarrow$ 以上六种原语覆盖所有执行模式。

---

## 4. 资源代数

**公理 4.1（资源分配与释放是成对的）**  
每个资源 $r$ 有分配点 $\alpha(r)$ 和释放点 $\zeta(r)$，且 $\alpha$ 在 $\zeta$ 之前，$\zeta$ 执行后 $r$ 不可访问。

**定义 4.1（资源类）**  
$R = (\tau, \alpha, \zeta, \gamma)$，其中 $\tau \in \{static, dynamic, os, thread, process\}$。

> **定理 4.1（资源生命周期单调性）**  
> $\text{公理 4.1} + \text{定义 4.1} \Rightarrow$ 在 `init → process* → cleanup` 序列中：init 前不存在，init 后存在，cleanup 后消失。

**定义 4.2（静态资源）**  
资源 $r$ 被称为静态的 $\iff \gamma(r) = permanent \iff \zeta(r) = \emptyset$。

> **定理 4.2（静态资源的不可回收性）**  
> $\text{定义 4.2} \Rightarrow$ 静态资源生命周期等于进程生命周期。  
> *Rust 映射：代码段、类型元数据、vtable、工厂注册信息，编译期固定。*

---

## 5. 组合与范畴结构

**公理 5.1（串行组合操作的存在性）**  
给定 $M_1: I \to O$ 和 $M_2: O \to J$，存在组合 $M_1 ⨟ M_2: I \to J$。

**定义 5.1（串行组合）**  
$M_1 ⨟ M_2 = (S_1 \times S_2, I_1, O_2, \delta_{12}, \rho_{12})$，其中 $\delta_{12}$ 先执行 $\delta_1$ 再执行 $\delta_2$。

> **定理 5.1（确定性保持）**  
> $\text{定义 5.1} \Rightarrow$ $M_1$ 和 $M_2$ 都确定 $\implies$ $M_1 ⨟ M_2$ 确定。  
> *证明：确定性函数的复合仍然是确定性函数。*

**定义 5.2（机器范畴 $\mathcal{M}$）**  
对象：类型 $I, O$。态射：机器 $M: I \to O$。恒等态射：$id_I = (\emptyset, I, I, \emptyset, \delta_{id}, \rho_{id})$。组合：$⨟$。

> **定理 5.2（$\mathcal{M}$ 满足范畴律）**  
> $\text{定义 5.2} \Rightarrow$  
> 1. 封闭性：$⨟$ 的输出类型匹配  
> 2. 结合律：$(M_1 ⨟ M_2) ⨟ M_3 = M_1 ⨟ (M_2 ⨟ M_3)$  
> 3. 单位律：$id ⨟ M = M ⨟ id = M$

---

## 6. 部署代数

**公理 6.1（抽象与物理可分离）**  
同一 Machine 的语义行为 $\delta$ 不依赖于其在物理层如何被执行。

**定义 6.1（部署映射）**  
$\Delta: M \to (Hint \times Spec)$ 将 Machine 映射到执行原语和参数。

> **定理 6.1（部署不变性）**  
> $\text{公理 6.1} + \text{定义 6.1} \Rightarrow$ 任意部署映射 $\Delta$ 不改变 $\delta$。

> **定理 6.2（部署一致性）**  
> $\text{定理 6.1} \Rightarrow$ 同一 $M$ 可在不同部署中使用不同 $Hint$，其 $\delta$ 一致。

---

## 7. 系统整体定理

> **定理 7.1（系统封闭性）**  
> $\text{定义 2.4} \Rightarrow$ 任意 $M \in M_\Sigma$ 的 $\delta$ 调用只读取：$S_M$、$L_\Sigma$ 中的上游数据、当前输入 $i$。

> **定理 7.2（可观测性完备性）**  
> $\text{定义 2.4} \Rightarrow$ $O_M$ 中标注为 FlowKind::Observe 的输出可达收集器 $\iff$ $L_\Sigma$ 包含对应连接。

> **定理 7.3（背压传播条件）**  
> $\text{LinkKind 定义} \Rightarrow$ 背压传播 $\iff$ 连接使用 BoundedBuf_{blocking}。

---

## 7.5 工程修补：数学模型与实现之间的缝隙

> 以下各条承认图灵机/Mealy 机形式化定义未覆盖、但工程实现必须处理的边界条件。每条标注了对应的 Rust 修补机制。

**修补 7.5.1（连接计数）**  
数学定义 2.4 中连接 $\ell$ 的存在性是布尔量。早期工程实现用引用计数（`AtomicUsize`）跟踪活跃连接数并暴露 `output_is_connected()`；该机制已**整体移除**——连接存在性由部署层拓扑决定（`DeploySpec` 在 `materialize` 时一次性建立），运行时不跟踪计数，Machine 不查询输出可达性（观测短路是 runtime 职责）。此修补条目保留作为设计历史。

**修补 7.5.2（panic 时的状态清理）**  
数学定义 1.2 中 $\delta$ 是全函数（对所有输入都有定义）。工程实现中 $\delta$（即 `process()`）可能 panic。此时状态 $S$ 处于未定义中间态，调用 $\rho$（`cleanup()`）可能不安全。

**修补方式**：线性运行时使用 `CleanupGuard` 在 panic 时安全丢弃状态（跳过 `cleanup`）；异步运行时需等价机制。这是"safe but leaky"的折衷——资源可能泄漏，但不会产生未定义行为。

**修补 7.5.3（信号传递的类型擦除）**  
数学定义中系统信号 $\sigma \in \{Shutdown, Checkpoint\}$ 是离散事件。`send_signal(&self, signal: SystemSignal)` 携带信号类型；`poll_signal() -> Option<SystemSignal>` 只消费 `Checkpoint`——`Shutdown` 由 runtime 经 `has_shutdown_signal()` peek 强制执行（停机是 runtime 生命周期职责，不由机器消费）。

**修补 7.5.4（Source 的常量注入）**  
数学定义中 Source 是 $M = (S, \emptyset, O, \delta, \rho)$，$\delta: S \times \emptyset \to S \times O$。工程实现中 `init()` 只能从 `MachineContext` 获取信息，无法接受外部配置参数来设定要产生的常量值。

**修补方式**：通过 `MachineContext` 的配置通道（如 `config_overrides`）传入序列化值，或通过部署时的 `MachineInstance` 配置注入。

**修补 7.5.5（部署校验的完备性）**  
数学定义 2.4 中连接图 $\Sigma$ 的合法性要求：所有连接的端口名存在、类型匹配、无循环依赖。工程实现的 `DeploySpec::validate()` 只检查了机器名存在性，端口名和类型检查依赖运行时 `LinkCompat::check`，循环依赖检查未实现。

**修补方式**：在 `validate()` 中补充端口名存在性检查和类型匹配检查；循环依赖检查需拓扑排序算法。

> **v3 更新**：循环依赖检查已在 `DynamicTopology` 中通过 **Kahn 算法**完整实现（`detect_cycle()` 返回环路路径）。每次 `Link` 操作前执行检查，`apply_batch()` 批量操作同样检查。`DeploySpec::validate()` 中的静态检查仍待实现。

**修补 7.5.6（Moore 型的首次输出）**  
见工程修补 1.2a。Moore 型 Machine 首次调用时状态为初始值 $s_0$，若 $\lambda(s_0)$ 无意义，需约定输出 `Idle`。这是形式化定义不涉及但实现必须处理的边界。

---

## 8. Rust 映射

| 代数概念 | Rust 实现 | 编译器保证 |
|---------|-----------|-----------|
| 纯函数 $f = (I, O, \hat{f})$ | `trait Func { type I; type O; fn call(I) -> O }` | Send+Sync，无 &mut State |
| 机器 $M = (S, I, O, \delta, \rho)$ | `trait Machine { type State; type Input: HasPortInfo; type Output: HasPortInfo; type Ports: PortSet; type ProcessOutput: MachineOutput<Self::Output>; process(); cleanup() }` | Send+Sync，生命周期间 |
| 接口集 $\Gamma$ | `type Input`/`type Output`（enum，每端口一个 variant） | HasPortInfo 保证端口元数据可查 |
| 端口集连接 | `type Ports: PortSet<Input=Self::Input, Output=Self::Output>` | PortSet 保证类型空间与值空间一致 |
| 实体 $E = (S, name)$ | `trait Entity { type S; fn name() }` | 无 process，无端口 |
| 端口 $p = (T, d, f)$ | `PortDecl { type_id, dir: PortDir, flow: FlowKind }` + enum variant | TypeId 连接时检查 |
| 连接 $\ell$ | `LinkSpec { out, into, kind: LinkKind }` | LinkCompat::check |
| 连接图 $\Sigma$ | `DeploySpec { machines, links }` | validate() |
| 部署 $\Delta$ | `MachinePhysicalSpec { execution: ExecutionHint }` | Trait 签名不含 Hint |
| 资源类 $R$ | `ResourceClass { Static, DynamicHeap, OsResource, ... }` | 文档标记 |
| 恒等态射 $id$ | `builtin::Identity<I>` | 零开销，零分支 |
| 范畴组合 $⨟$ | `FuncScratchPipeline<(A,B)>` | 编译期泛型复合 |
| 多端口扇出 | `MultiOutput::YieldMulti(Vec<O>)` / `TupleOutput::Yield(O, O)` | 定理 2.4 |
| 输出可达性 | 部署层拓扑（`DeploySpec`/`materialize`） | 定理 2.5 |
| 时间 $t$ | `TimeTick { ns: u64 }` / `MachineContext::time_tick()` | 纳秒精度，无毫秒回退 |
| 会话类型 $T$ | `SessionType { ops: Vec<SessionOp> }` | 定理 11.1（二元对偶） |
| 全局类型 $G$ | `GlobalType { ops: Vec<GlobalOp> }` | 定理 11.2（通信安全性） |
| 局部类型 $L_p$ | `LocalType { ops: Vec<LocalOp> }` | 定理 11.3（进展性） |
| 投影 $\text{project}$ | `project(global, role) -> LocalType` | 定义 11.4 |
| 混合状态 $S_c \times S_d$ | `HybridState<C, D>` | 定义 12.1 |
| 连续演化 $f$ | `HybridMachine::flow(c, dt, d) -> C` | 定义 12.2 |
| 守卫 $g$ | `HybridMachine::guard(c, d) -> Option<Jump<D>>` | 定义 12.3 |
| 跳变 $j$ | `Jump<D> { Transition, Reset, Emit }` | 定义 12.4 |
| 生命周期状态 $l$ | `struct Init/Running/Stopping/Stopped`（密封 ZST） | 定理 13.1（编译期安全性） |
| 类型状态句柄 | `MachineHandle<M, S: LifecycleState>` | 定理 13.2（线性性） |

---

## 9. Curry-Howard 对应

| 范畴论 | 类型论 | axiom |
|--------|--------|-------|
| 对象 $I, O$ | 类型 `I`, `O` | `type Input, type Output` |
| 态射 $M: I \to O$ | 函数 $I \to O$ | `trait Machine` |
| 恒等态射 | `identity` | `builtin::Identity<I>` |
| 组合 $⨟$ | 函数复合 | `FuncScratchPipeline` |
| 积 $S_1 \times S_2$ | 元组 `(S1, S2)` | 组合 Machine 的 State |
| 初始对象 | `!` (empty, never) | `builtin::EntityRoot`（无端口、无 process） |

---

## 10. 与 V8 对比

| V8 定理 | axiom 对应 | 改进 |
|---------|-----------|------|
| 定理 7：生命周期单调性 | 定理 4.1 | 显式资源分类；不可回收资源标记为 Static |
| 定理 8：注册后封闭性 | 定理 7.1 | 显式定义封闭来源为 $L_\Sigma$ |
| — | 定理 1.1 | 新增：纯函数物理隔离（并行安全性） |
| — | 定理 1.3 | 新增：实体可观测性（无 process 的持久存在） |
| — | 定理 2.2 | 新增：观测隔离性（Obs 不在 $\delta$ 输入中） |
| — | 定理 5.2 | 新增：范畴律验证（组合的代数结构） |
| — | 定理 6.1 | 新增：部署不变性（抽象与物理可分离） |
| — | 公理 1.1-3.2 | 显式公理化——所有推论有据可查 |
| — | 恒等态射 | 具体化为 `builtin::Identity<I>` |
| — | 初始对象 | 具体化为 `builtin::EntityRoot` |

---

## 11. 会话类型

### 11.1 二元会话类型

**定义 11.1（会话类型）**  
一个会话类型 $T$ 是以下递归文法：
$$T ::= \ !\ell.T \ \mid\ ?\ell.T \ \mid\ \mu t.T \ \mid\ t \ \mid\ \text{end}$$

其中 $!\ell.T$ 表示发送标签 $\ell$ 后继续为 $T$，$?\ell.T$ 表示接收标签 $\ell$ 后继续为 $T$，$\mu t.T$ 是递归类型，$\text{end}$ 是终止。

**定义 11.2（二元对偶）**  
两个会话类型 $T_1$ 和 $T_2$ 是对偶的（$\text{dual}(T_1, T_2)$），当且仅当：
- $\text{dual}(!\ell.T_1, ?\ell.T_2) \iff \text{dual}(T_1, T_2)$
- $\text{dual}(\text{end}, \text{end})$

> **定理 11.1（二元连接安全性）**  
> $\text{定义 11.2} \Rightarrow$ 两个端口连接安全 $\iff$ 其会话类型对偶。  
> *Rust 映射：`session::is_dual(&T1, &T2)`。*

### 11.2 多方会话类型（MPST）

**定义 11.3（全局类型）**  
全局类型 $G$ 描述所有参与者之间的交互编舞：
$$G ::= \ p_1 \to p_2 : \ell. G \ \mid\ \text{end} \ \mid\ \text{skip}$$

其中 $p_1 \to p_2 : \ell$ 表示角色 $p_1$ 向角色 $p_2$ 发送标签 $\ell$。

> **Rust 映射：** `GlobalType { ops: Vec<GlobalOp> }`，`GlobalOp::Message { from, to, label }`。

**定义 11.4（投影）**  
全局类型 $G$ 在角色 $p$ 上的投影 $\text{project}(G, p)$ 产生局部类型 $L_p$：
- $p_1 \to p_2 : \ell. G'$：
  - 若 $p = p_1$：$L = !\ell \to p_2. \text{project}(G', p)$
  - 若 $p = p_2$：$L = ?\ell \leftarrow p_1. \text{project}(G', p)$
  - 否则：$L = \text{skip}. \text{project}(G', p)$

> **Rust 映射：** `project(global: &GlobalType, role: &str) -> LocalType`。

> **定理 11.2（通信安全性）**  
> $\text{定义 11.3} \land \text{定义 11.4} \Rightarrow$ 消息总是发送给期望接收它的角色。  
> *证明：投影保证了发送方的 `Send{to}` 和接收方的 `Recv{from}` 在全局类型中对应同一个 `Message{from, to}`。*

> **定理 11.3（进展性）**  
> $\text{定义 11.3} \land \text{定义 11.4} \Rightarrow$ 若所有参与者遵循其投影的局部类型，协议不会死锁。  
> *证明：全局类型描述了一个线性的交互序列，投影保持了顺序约束。*

---

## 12. 混合系统

### 12.1 混合自动机模型

**定义 12.1（混合状态）**  
混合状态是连续状态与离散状态的积：
$$S = S_c \times S_d$$

> **Rust 映射：** `HybridState<C, D> { continuous: C, discrete: D }`。

**定义 12.2（连续演化/流）**  
在两次离散跳变之间，连续状态通过 ODE 演化：
$$\frac{dc}{dt} = f(c, d)$$

其中 $d$ 在演化期间保持不变。

> **Rust 映射：** `HybridMachine::flow(c: &C, dt: f64, d: &D) -> C`。

**定义 12.3（守卫条件）**  
守卫条件 $g: S_c \times S_d \to \text{Option<Jump>}$ 检测连续状态是否越过阈值，触发离散跳变。

> **Rust 映射：** `HybridMachine::guard(c: &C, d: &D) -> Option<Jump<D>>`。

**定义 12.4（跳变）**  
跳变 $j$ 是瞬时状态转换：
- $\text{Transition}(d')$：离散状态变为 $d'$，调用 `reset()` 更新连续状态
- $\text{Reset}\{d'\}$：离散状态变为 $d'$，连续状态通过 `reset()` 重置
- $\text{Emit}(s)$：发射输出，不改变状态

> **Rust 映射：** `Jump<D>` 枚举。

**定义 12.5（混合转移函数）**  
混合系统的转移函数：
$$\delta_h: (S_c, S_d) \times I \times \Delta t \to (S_c, S_d) \times O$$

其中 $\Delta t$ 来自运行时的 `TimeTick`（纳秒精度）。

> **定理 12.1（时间精度保持）**  
> $\text{定义 12.5} \Rightarrow$ `HybridDriver` 使用 `TimeTick`（纳秒）计算 $\Delta t$，不损失精度。  
> *Rust 映射：`step_to_tick(tick: TimeTick)` 自动计算 `dt`。*

> **定理 12.2（跳变的原子性）**  
> $\text{定义 12.4} \Rightarrow$ 跳变在 `apply_pending_jumps()` 中原子应用——离散状态更新和 `reset()` 调用在同一步完成。

---

## 13. 生命周期类型状态

### 13.1 类型状态模式

**定义 13.1（生命周期状态集）**  
机器的生命周期状态集 $\mathcal{L} = \{\text{Init}, \text{Running}, \text{Stopping}, \text{Stopped}\}$，带有偏序关系：
$$\text{Init} \prec \text{Running} \prec \text{Stopping} \prec \text{Stopped}$$

**定义 13.2（类型状态编码）**  
每个生命周期状态 $l \in \mathcal{L}$ 对应一个零大小类型（ZST），作为 `MachineHandle<M, S>` 的类型参数 $S$。

> **Rust 映射：** `struct Init; struct Running; struct Stopping; struct Stopped;`（密封于 `LifecycleState` trait）。

**定义 13.3（状态转换函数）**  
转换函数 $\text{trans}: \text{MachineHandle}<M, l_1> \to \text{MachineHandle}<M, l_2>$ 仅在 $l_1 \prec l_2$ 时存在：
- $\text{start}: \text{Init} \to \text{Running}$
- $\text{stop}: \text{Running} \to \text{Stopping}$
- $\text{finish}: \text{Stopping} \to \text{Stopped}$

> **定理 13.1（编译期安全性）**  
> $\text{定义 13.2} \land \text{定义 13.3} \Rightarrow$ 非法状态转换在编译期被拒绝。  
> *证明：Rust 类型系统保证 `process()` 方法仅存在于 `MachineHandle<M, Running>` 和 `MachineHandle<M, Stopping>` 上，`cleanup()` 仅存在于 `MachineHandle<M, Stopped>` 上。编译器拒绝在错误状态调用方法。*

> **定理 13.2（线性性保证）**  
> $\text{定义 13.3} \Rightarrow$ 每个转换消耗 `self`（按值接收），返回新状态的句柄。旧状态的句柄不可再用。  
> *Rust 映射：`fn start(self) -> MachineHandle<M, Running>`。*

> **定理 13.3（密封性）**  
> $\text{定义 13.2} \Rightarrow$ 外部代码不能引入新的生命周期状态。  
> *Rust 映射：`LifecycleState: private::Sealed`，`Sealed` trait 是私有模块。*

---

## 14. 统一性评估：研究脉络的归约与覆盖

本节回答一个元问题：axiom 的少数原语能否吸收数十年并发/分布式/控制论研究的多条脉络，形成一个统一的数学形式化体系？评估分两部分：**已吸收脉络的归约表**，与**未覆盖空白的判断**。

### 14.1 已吸收研究脉络的归约

下表列出 axiom 已形式化吸收的研究脉络，每条都归约到 axiom 的少数原语（Port / Flow / Session / Topology / Lifecycle / Machine）。归约的方向是：原研究的概念在 axiom 中**消失**为一个组合或特化，而非新增并列概念。

| 研究脉络 | 代表工作 | axiom 归约 | 吸收方式 |
|----------|----------|------------|----------|
| 二元会话类型 | Honda 1993, Takeuchi 1994 | `SessionType` + `is_dual`（§11.1） | 协议成为 Port 的属性，对偶检查成为 `can_link_to` 的一步 |
| 多方会话类型 (MPST) | Honda–Yoshida–Carbone 2008/2016 | `GlobalType` + `project` + `is_consistent`（§11.2） | 全局编舞投影为局部类型，一致性检查归约为"投影后发送/接收配对" |
| 接口自动机 | de Alfaro–Henzinger 2001 | `can_link_to` + `LinkCompat`（§2） | 兼容性检查从独立模型变为 Port 四维（方向/流/类型/协议）的合取 |
| 混合自动机 | Alur–Courcoubetis–Henzinger 1995 | `HybridMachine` + `HybridDriver`（§12） | 连续动力学作为 Machine 的扩展 trait，Jump 复用既有输出通道 |
| Typestate / 线性类型 | Strom–Yemini 1986, Rust ownership | `MachineHandle<M, S>` + sealed ZST（§13） | 生命周期相位成为类型参数，转换消耗 self → 编译期安全性 |
| 数据流 / Kahn 网络 | Kahn 1974 | `Machine` + `Port` + 拓扑约束（§1–3） | 数据流网络 = 无环 Port 图；Kahn 不动点 = 拓扑顺序的稳定迭代 |
| 拉模型流式处理 | iteratees / FRP（Elm, Rx） | `StreamingMachine::process_stream`（§1.5 补充） | 惰性迭代器输出：首次 `next()` 重置游标，机器内部分批产出——推拉双模型 |
| 零拷贝输入 | zero-copy 框架（io_uring, DPDK） | `FuncRef::call_ref(&Input)`（§5 补充） | 借用输入零分配 vs 拥有输入两条路径并存——零成本不是"不用付出"，是"不额外付出" |
| 动态拓扑 / 热切换 | —— | `DynamicTopology` + `apply_batch` + Replace（§6） | 工程能力（非研究脉络）：为纯数据 `DeploySpec` 的运行时镜像（契约完整性）而存在；运行时重组归约为对 (machines, links) 的原子快照+回滚。**静态拓扑是默认世界观**（见下方定位注记） |
| 范畴组合 | —— | `Func` 复合 + `⨟`（§5） | 函数复合 = 范畴态射复合，编译期泛型零成本 |
| Curry-Howard | —— | 定理↔类型映射（§9） | 命题即类型，证明即程序，直接对应 Rust 类型系统 |

**归约的数学含义**：axiom 没有为上述任何一条脉络引入"并列的并行 universe"。每条脉络都被表达为几个原语的**约束**或**扩展**：
- 会话类型 = Port 的协议约束（不新增连接概念）
- 混合系统 = Machine 的连续扩展（不新增计算模型）
- typestate = MachineHandle 的类型参数（不新增运行时对象）
- 动态拓扑 = (machines, links) 的原子变更（不新增图模型）

这是"统一"的判据：**概念数量不随研究脉络增加而膨胀**。

> **定位注记（动态拓扑）**：`DynamicTopology` 与上表其他条目不同——它不是研究
> 脉络（代表工作为空），而是工程能力。它存在的理由是**契约完整性**：`DeploySpec`
> 是纯数据、可序列化，因此拓扑的运行时镜像必须有一个类型存在。axiom 的默认
> 世界观是**静态拓扑 + 部署期验证**（`validate_deep` + `analysis` 的检查——度
> 约束、Inline 无环、Moore 循环安全——只在拓扑固定时有意义）。绝大多数看似
> "动态"的场景（弹性伸缩、热切换、会话子图）可由**静态拓扑 + 控制/状态变化**
> 表达（如部署时预分配最大副本数、运行时用控制端口启停）；运行时重组是可选
> 能力，非核心代数。真正需要它的只剩少数场景（运行时决策的拓扑、会话级私有
> 子图的动态生命周期），且**类型空间恒静态**——只能增减已注册类型的实例。

### 14.2 未覆盖空白与判断

下表列出常被提及但 axiom **尚未形式化**的研究脉络，以及是否应填补的判断。

| 研究脉络 | 代表工作 | 与 axiom 的关系 | 判断 |
|----------|----------|----------------|------|
| π-calculus | Milner 1992 | 通道的动态创建与移动 | **暂不填补**。axiom 的 Port 名是 `&'static str`，通道不是一等公民。动态创建通过 `DynamicTopology::Spawn` 实现，但语义是拓扑变更而非通道移动。若未来需要移动语义，可作为 `TopologyOp` 的扩展，不需要新原语。 |
| Petri net | Petri 1962 | 并发 token 与库所 | **暂不填补**。Petri net 的并发语义可由"多端口扇出 + 多输入聚合"组合表达（定理 2.4 多端口扇出）。Petri net 的 token 数量约束目前无对应；可作为 `PortDecl` 的可选容量约束，但收益有限。 |
| CSP / CCS | Hoare 1978, Milner 1980 | 进程代数与同步通信 | **暂不填补**。CSP 的同步 rendezvous 与 axiom 的异步端口不同。会话类型已覆盖"协议级同步"；CSP 的代数等价定律目前无对应，但 axiom 目标是工程可执行系统而非进程代数演算。 |
| Actor 模型 | Hewitt 1973 | 位置透明的消息传递 | **部分覆盖**。`Machine` 即 actor，Port 即 mailbox。未覆盖的是位置透明（远程 actor）。这属于传输层问题，可在 runtime 层引入 `RemotePort` 而不影响核心代数。 |
| 时态逻辑 / LTL | Pnueli 1977 | 系统性质的时态规约 | **暂不填补**。axiom 的定理是结构性（安全性/进展性），时态性质验证需要模型检查器。可作为外部工具（如 TLA+ 风格）对 axiom 拓扑做验证，不需要嵌入核心代数。 |
| 线性时序逻辑 (LTL) 模型检查 | Clarke 1981 | 自动化性质验证 | 同上。 |
| 软实时调度 | Liu–Layland 1973 | 截止期与调度 | **部分覆盖**。`TimeTick` 提供纳秒时间，`Lifecycle::Stopping` 支持优雅退出。未覆盖调度算法本身——这属于 runtime 策略，核心代数不涉及。 |

### 14.3 统一性结论

> **定理 14.1（原语收敛性）**  
> axiom 以 6 类原语（Port / Flow / Session / Topology / Lifecycle / Machine）吸收了 9 条研究脉络（§14.1），且每条脉络的表达不引入新的并列概念，而是既有原语的约束或扩展。

> **定理 14.2（空白可扩展性）**  
> §14.2 列出的未覆盖脉络中，无一需要新增核心原语。它们的填补路径要么是"既有原语加可选约束"（如 Petri net 容量），要么是"runtime 层扩展"（如远程 actor），要么是"外部工具验证"（如时态逻辑）。核心代数保持稳定。

**结论**：axiom 已具备一个统一数学形式化体系的**骨架**——少数原语 + 可组合扩展。已吸收 9 条脉络，未覆盖的 7 条均有明确的、不破坏核心代数的填补路径。体系的"统一性"体现为**概念数量的收敛**：无论吸收多少研究脉络，核心原语始终是 6 类，新增脉络以约束或扩展形式归约进来，而非并列堆积。

**下一步建议**（若需进一步深化）：
1. **π-calculus 移动语义**：若未来需要通道移动，研究 `TopologyOp::MovePort` 的语义。
2. **时态性质验证**：提供 axiom 拓扑 → TLA+ 的导出器，让外部模型检查器验证时态性质。
3. **Petri net 容量约束**：为 `PortDecl` 增加可选 `capacity: Option<usize>`，覆盖有界库所语义。

这三项都是**可选扩展**，不影响当前统一性结论。

---

## 15. 零成本抽象：抽象层与物理层的解耦

> 本节形式化 axiom 的核心设计公理：**抽象层与物理层是两个独立的存在层级，抽象为物理提供正确性约束，物理不为抽象的存在支付任何代价**。这是 Rust 零成本抽象哲学在系统架构层的对应。

### 15.1 两个存在层级

**定义 15.1（抽象层 $\mathcal{A}$）**  
抽象层是 axiom 的类型与拓扑代数：
$$\mathcal{A} = (\mathcal{M}, \mathcal{P}, \mathcal{L}, \mathcal{T}, \Sigma)$$
其中 $\mathcal{M}$ 为 Machine 集合，$\mathcal{P}$ 为 Port 集合，$\mathcal{L}$ 为 Link 集合，$\mathcal{T}$ 为类型集合，$\Sigma \subseteq \mathcal{M} \times \mathcal{M}$ 为连接图。$\mathcal{A}$ 的元素是**数学对象**：模块、端口、数据流、控制流——它们组织系统的可读性与可推理性。

**定义 15.2（物理层 $\mathcal{P}_h$）**  
物理层是 CPU、栈、指令集、缓存线、地址空间：
$$\mathcal{P}_h = (L, T, \Phi, \text{mem})$$
其中 $L$ 为内存位置集（公理 0.1），$T$ 为线程集，$\Phi$ 为指令序列，$\text{mem}: L \to V$ 为内存状态。$\mathcal{P}_h$ 的元素是**物理实体**：地址、寄存器、栈帧——它们执行实际计算。

**公理 15.1（存在层级的不相交性）**  
$$\mathcal{A} \cap \mathcal{P}_h = \emptyset$$
抽象层的对象（"模块""端口""控制流"）在物理层**不存在**。物理层只有内存地址与指令。当说"模块 $M$ 向模块 $N$ 发送数据"时，物理层发生的仅仅是：某线程向某地址写入字节，另一线程从该地址读取字节。"模块""发送"这些标签是抽象层的语义注解，物理层对其无感知。

**例 15.1（接收-打印系统的双层级描述）**  
两个线程：$T_1$ 负责接收网络数据，$T_2$ 负责计算并打印。

| 层级 | 描述 |
|------|------|
| 抽象层 $\mathcal{A}$ | 接收模块 $M_R$ →（数据流）→ 打印模块 $M_P$；若加入持久化 $M_S$，则 $M_R$ 扇出到 $M_P$ 与 $M_S$，形成一个有向图。 |
| 物理层 $\mathcal{P}_h$ | $T_1$ 执行 `recv()` 写入缓冲区 $B$（位于 $L_B$）；$T_2$ 从 $B$ 读取，执行 `write(stdout)`。"模块""数据流"在物理层不存在。 |

抽象层的图结构 $M_R \to M_P$ 与物理层的内存写入是同一现象的两个描述，前者服务于人类推理，后者是机器实际执行。

### 15.2 抽象对物理的零侵入公理

**公理 15.2（零成本抽象）**  
对任意抽象层结构 $\alpha \in \mathcal{A}$，若其物理实现 $[\![\alpha]\!] \in \mathcal{P}_h$ 满足以下条件，则称 $\alpha$ 是**零成本的**：

1. **运行时存在性消失**：$\alpha$ 在编译产物中不分配任何数据结构。即不存在运行时对象 $o$ 使得 $\text{typeof}(o) = \text{typeof}(\alpha)$。
2. **执行时间不变**：$[\![\alpha]\!]$ 的执行时间等于等价的手写物理实现 $h_\alpha$ 的执行时间：$t([\![\alpha]\!]) = t(h_\alpha) + \epsilon$，其中 $\epsilon$ 为编译器优化的噪声（通常 $|\epsilon| / t(h_\alpha) < 0.05$）。
3. **内存占用不变**：$[\![\alpha]\!]$ 的稳态内存占用等于 $h_\alpha$ 的内存占用。

**定理 15.1（抽象的编译期消解）**  
若抽象 $\alpha$ 满足以下条件，则 $\alpha$ 是零成本的：
- (a) $\alpha$ 的所有类型参数在编译期已知（**静态部署**）；
- (b) $\alpha$ 的所有转换函数是纯函数且被编译器内联；
- (c) $\alpha$ 不引入运行时类型擦除（无 `Box<dyn Any>`、无 vtable）。

> **证明**：  
> (a) $\Rightarrow$ 编译器对 $\alpha$ 的每个实例进行单态化（monomorphization），生成具体类型的代码，不生成 trait object；  
> (b) $\Rightarrow$ 内联后转换函数消失为指令序列，与手写代码同构；  
> (c) $\Rightarrow$ 无堆分配、无动态分派，$t([\![\alpha]\!]) = t(h_\alpha)$。  
> 由公理 15.2，$\alpha$ 是零成本的。$\square$

### 15.3 axiom 的双部署路径

axiom 提供**两条部署路径**，分别对应公理 15.1 的两种满足方式：

| 路径 | 函数 | 拓扑已知于 | 每条消息成本 | 满足公理 15.2？ | 用例 |
|------|------|-----------|-------------|----------------|------|
| **静态** | `axiom_runtime::static_path::{pipeline2, pipeline3, fanout2, fanin2}` | 编译期 | **零** | 是 | 固定管道、扇出/扇入、热路径（**默认**） |
| **动态** | `Runtime::materialize(spec)` | 运行时 | ~5x（堆分配+动态分派） | 否 | 运行时拓扑镜像（极少数场景） |

> **静态优先原则**：拓扑在编译/部署期已知是**默认**。动态路径（`Runtime::materialize`）仅在
> 拓扑必须由运行时决策决定时使用（推论 15.3a），且**类型空间恒静态**——
> 运行时只能增减已注册 `machine_type` 的实例，无法加载新代码（插件代码加载
> 是 runtime-adapter 的关切，非 axiom core 契约）。多数"动态"场景（伸缩、
> 热切换、会话子图）可由静态拓扑 + 控制/状态变化表达。

> **结构作用域约束（反窄化规则）。** axiom 的结构层必须将**任意拓扑**——
> 多流水线、扇出、扇入、有向环、嵌套复合——作为一等静态定义表达。单一线性
> 流水线 $A \to B \to C$、单一执行线程、单一固定功能路径均为设计空间的**窄
> 子集**，**不是**默认能力，**不是**目标，也**不是**当完整结构能力难以实现
> 时的可接受回退。当某特性"对线性链容易、对一般 DAG 困难"时，正确做法是
> **解决一般情形，或明确标记为超出范围**——而非静默地将 axiom 窄化至容易
> 的子集。窄子集可作为最小验证探针（如 §15.3 的 `pipeline3` 零开销探针），
> 但不得被误认为 axiom 的能力上限。

**定义 15.3（静态部署 `deploy_static`）**  
静态部署将拓扑编码在类型参数 `<Src, Dst, L>` 中：

$$\text{deploy\_static}: \text{Machine} \times \text{Machine} \times \text{Link} \to \text{StaticDeployed}$$

其中 $\text{Link}\langle S, D \rangle$ 是编译期已知的纯函数 `extract: S::Output → Option<D::Input>`。

> **实现映射**：$\text{deploy\_static}$ 的实际实现为 `axiom_runtime::static_path`
> 模块——`pipeline2`/`pipeline3`（线性）、`fanout2`（扇出，经 `Split`）、
> `fanin2`（扇入，经 `Merge`）。类型契约（`Link`/`Split`/`Merge`）定义于
> `axiom::static_exec`。以下形式化中 $\text{deploy\_static}$ 指该模块的统称。

其物理实现 $[\![\text{deploy\_static}]\!]$ 满足定理 15.1 的三条：
- (a) 拓扑 $\langle Src, Dst, L \rangle$ 编译期已知 → 单态化；
- (b) `Link::extract` 是 trait 方法，被编译器内联为 `match out { ... => Some(...) }`；
- (c) 通道类型为具体 `mpsc::Sender<Src::Output>`，无类型擦除。

**定理 15.2（静态部署的零成本性）**  
$\text{deploy\_static}$ 满足公理 15.2，即：
$$t([\![\text{deploy\_static}\langle S, D, L \rangle]\!]) = t(h_{S \to D}) + \epsilon$$
其中 $h_{S \to D}$ 是等价的手写两 task + mpsc channel 实现。

> **经验验证（L1 基准，参考环境 release 构建，100k 条消息）**：相对吞吐（绝对数值随机器/分配器而异，排序关系环境无关）：
> - 手写（`l1_pipeline`）：1.0×（基线）
> - 静态路径（`l1_static`）：**1.24×**（因为 `Link::extract` 内联消除了手写版的 adapter task）
> - 动态路径（`l1_declarative`）：0.20×（即"动态税"）
>
> 静态路径不仅追平手写，反而**更快**——因为抽象层让编译器看到了原本手写代码隐藏的转换结构，从而能消除一个中间 task。这正面验证了公理 15.1：抽象层的存在没有为物理层增添负担，反而启发了物理优化。

**定义 15.4（动态部署 `deploy_dynamic`）**  
动态部署在运行时接受 `DeploySpec`，通过 `Wire { payload: Box<dyn Any> }` 信封类型擦除每条消息：

$$\text{deploy\_dynamic}: \text{DeploySpec} \times \text{FactoryMap} \to \text{Deployed}$$

其物理实现不满足定理 15.1：
- (a) 拓扑运行时已知 → 无法单态化；
- (b) `from_port_name` 通过字符串匹配重建类型 → 无法内联；
- (c) `Box<dyn Any>` 每条消息堆分配 → 违反 (c)。

**定理 15.3（动态税的不可避免性）**  
若拓扑在运行时方可确定，则每条消息至少需要：
- 1 次堆分配（承载类型擦除的载荷）；
- 1 次动态分派（`from_port_name` 重建类型）；
- 1 次字符串比较（识别目标端口）。

即 $\text{deploy\_dynamic}$ 不可能是零成本的。这是数学上的必然，非实现缺陷。

> **推论 15.3a（动态路径的正当性）**  
> 动态税是**运行时拓扑灵活性**的对价。当且仅当拓扑必须由配置、插件或运行时决策决定时，支付此税是合理的；否则应使用静态路径。

### 15.4 抽象与物理的分离定理

**定理 15.4（抽象对物理的非侵入性）**  
设 $\Pi$ 为任意 runtime adapter（如 `axiom_tokio`、`axiom_rayon`），$\alpha \in \mathcal{A}$ 为任意 axiom 抽象结构。则：

$$\forall \Pi, \forall \alpha: \quad [\![\alpha]\!]_\Pi \text{ 的物理行为} = [\![\alpha]\!]_{\Pi'} \text{ 的物理行为} \quad \text{（在语义等价意义下）}$$

即抽象层 $\alpha$ 不依赖任何特定 runtime。runtime adapter 的替换不改变抽象层的语义，只改变物理层的执行策略（线程模型、调度器、IO 模型）。

> **Rust 映射**：axiom core crate（`src/`）不依赖 tokio、rayon 或任何 runtime。所有执行逻辑位于 adapter crate（`axiom-tokio/`、未来的 `axiom-rayon/`）。core 仅定义 trait 与类型，不包含 `async fn`、不调用 `spawn`、不分配线程。

**推论 15.4a（core 的纯抽象性）**  
axiom core 满足以下不变量：
- 无 `tokio::spawn`、无 `std::thread::spawn`；
- 无 `async fn`、无 `Future` 实现；
- 无运行时对象（无 executor、无 reactor）。

core 是 $\mathcal{A}$ 的形式化，adapter 是 $\mathcal{A} \to \mathcal{P}_h$ 的解释器。

### 15.5 类比：debug 与 release

**公理 15.3（抽象的开关性）**  
抽象层的存在不强制物理层感知它。这与 Rust 的 debug/release 二分相似：

| 维度 | debug build | release build |
|------|-------------|---------------|
| 边界检查 | 运行时执行 | 编译期证明后消除 |
| 调用栈 | 完整保留 | 尾调用优化、内联展开 |
| 抽象层 | 可观测（panic 信息、trait 名） | **消失**为机器码 |

axiom 的抽象在 release 构建中应同样**消失**：`Port`、`Link`、`Machine` 类型在运行时不存在，只存在具体类型与具体函数。这正是公理 15.2 条件 1（运行时存在性消失）的含义。

> **定理 15.5（抽象的开关可证性）**  
> axiom 的抽象在 release 构建中可被编译器完全消解，当且仅当满足定理 15.1 的三条条件。静态路径（`deploy_static`）满足，动态路径（`deploy_dynamic`）不满足（因 (c) 违反）。

### 15.6 设计公理总结

> **公理 15.4（axiom 的存在性公理）**  
> axiom 是抽象层 $\mathcal{A}$ 的形式化，不是运行时框架。其存在目的是：
> 1. 在抽象层提供**正确性约束**（类型可靠、端口一致、拓扑合法）；
> 2. 在物理层提供**零成本执行**（编译期消解、无运行时负担）；
> 3. 在两层之间提供**严格分离**（core 不含 runtime，adapter 不含抽象）。
>
> 当业务逻辑正确且编译通过，则物理执行正确，且无额外负担。这是 axiom 作为"架构公理"的本意——它约束设计，不约束执行。
