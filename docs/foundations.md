# axiom 代数基础

> **版本**: v2 · **日期**: 2026-06-21
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
8. [Rust 映射](#8-rust-映射)
9. [Curry-Howard 对应](#9-curry-howard-对应)
10. [与 V8 对比](#10-与-v8-对比)

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
> *Rust 映射：`ProcessOutput::YieldMulti(Vec<O>)` 允许一次返回多个 Output variant。*

**定义 2.4（连接图）**  
系统 $\Sigma = (M_\Sigma, L_\Sigma)$，$L_\Sigma \subseteq \bigcup_{M \in M_\Sigma} Out_M \times \bigcup_{M \in M_\Sigma} In_M$。

> **定理 2.5（输出可达性）**  
> $\text{定义 2.4} \Rightarrow$ Machine $M$ 的输出端口 $p$ 的数据可达 $\iff$ $\exists \ell \in L_\Sigma: \ell = (p, \_)$。  
> *Rust 映射：`MachineContext::output_is_connected()` 返回是否有消费者连接到输出端口。*

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

## 8. Rust 映射

| 代数概念 | Rust 实现 | 编译器保证 |
|---------|-----------|-----------|
| 纯函数 $f = (I, O, \hat{f})$ | `trait Func { type I; type O; fn call(I) -> O }` | Send+Sync，无 &mut State |
| 机器 $M = (S, I, O, \delta, \rho)$ | `trait Machine { type State; type Input: HasPortInfo; type Output: HasPortInfo; type Ports: PortSet; process(); cleanup() }` | Send+Sync，生命周期间 |
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
| 多端口扇出 | `ProcessOutput::YieldMulti(Vec<O>)` | 定理 2.4 |
| 输出可达性查询 | `MachineContext::output_is_connected()` | 定理 2.5 |

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
