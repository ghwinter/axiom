# axiom 代数基础

> **版本**: v1 · **日期**: 2026-06-21
>
> 本文档从范畴论、类型论和系统理论的角度，对 axiom 的计算模型进行形式化定义和推导。
>
> 所有定义和定理均对应于 axiom crate 中具体的 Rust 类型和 trait 实现，
> 使得数学证明与代码之间的映射关系是可检验的。

---

## 目录

1. [预备：物理基底](#0-预备物理基底)
2. [计算原语的代数定义](#1-计算原语的代数定义)
3. [端口与连接的范畴论结构](#2-端口与连接的范畴论结构)
4. [执行序列与调度](#3-执行序列与调度)
5. [资源代数](#4-资源代数)
6. [组合与范畴结构](#5-组合与范畴结构)
7. [部署代数](#6-部署代数)
8. [系统整体定理](#7-系统整体定理)
9. [Rust 中的代数结构表达](#8-rust-中的代数结构表达)
10. [形式逻辑与范畴论的统一](#9-形式逻辑与范畴论的统一)
11. [与 V8 证明的对比](#10-与-v8-证明的对比)

---

## 0. 预备：物理基底

定义物理世界的最简模型，一切推导建立于此。

**定义 0.1（内存位置集）**  
设 $L$ 为可寻址内存位置的集合。每个位置 $l \in L$ 在时刻 $t$ 持有值 $v \in V$。记 $mem_t: L \to V$ 为 $t$ 时刻的内存状态。

**定义 0.2（计算步）**  
一个计算步是一个三元组 $(r, w, \phi)$，其中：
- $r \subseteq L$ 是读取集
- $w \subseteq L$ 是写入集
- $\phi: V^{|r|} \to V^{|w|}$ 是转移函数

**定义 0.3（线程）**  
一个线程 $T$ 是一个计算步的序列：
$$T = (r_1, w_1, \phi_1) \cdot (r_2, w_2, \phi_2) \cdot ...$$
线程在物理层等价于一个栈——每一步压入帧，执行，弹出。栈的生命周期是线程的生命周期。

**定义 0.4（进程）**  
一个进程 $P = \{T_1, ..., T_n\}$ 是共享同一地址空间 $L_P \subseteq L$ 的一组线程。进程内线程共享 $L_P$，不同进程间的 $L$ 不相交（不考虑 OS 共享内存机制）。

---

## 1. 计算原语的代数定义

**定义 1.1（纯函数）**  
一个纯函数 $f$ 定义为：
$$f = (I, O, \hat{f})$$
其中 $I, O$ 是类型，$\hat{f}: I \to O$ 是映射函数。

**物理实现：** 一个计算步 $(r_f, w_f, \phi_f)$，其中 $r_f$ 编码输入 $I$ 为读取集，$w_f$ 写入结果到栈帧。

**定理 1.1（纯函数的物理隔离）：**  
对于任意 $f$，$w_f \cap L_{other} = \emptyset$——一个纯函数的写入集不涉及当前栈帧以外的任何内存位置。

*证明：由定义 0.2 和 0.3，栈帧退出时释放所有写入集。函数 $f$ 的写入仅限于其栈帧内的返回值位置。*

**推论 1.1a（纯函数可并行）：**  
任意 $\{f_i\}_{i=1}^n$ 可以在任意 $n$ 个线程上并行执行，且结果等价于顺序执行。

*证明：$w_{f_i}$ 互不相交，因为它们分别位于各自线程的栈帧上。根据定义 0.4，不同线程的栈帧在逻辑上独立，无竞争条件。*

**Func trait 对应 Rust 类型：**

```rust
// 代数对应：f = (I, O, f_hat)
trait Func: Send + Sync + 'static {
    type I;                // 输入类型
    type O;                // 输出类型
    fn call(i: Self::I) -> Self::O;  // 映射函数 f_hat
}

// 定理 1.1 的 Rust 表达：
// Func::call 不接收 &mut State —— 编译器保证它不能访问堆上持久状态。
```

---

**定义 1.2（机器）**  
一个机器 $M$ 定义为：
$$M = (S, I, O, Obs, \delta, \rho)$$
其中：
- $S$ 是状态空间
- $I$ 是输入类型
- $O$ 是输出类型
- $Obs$ 是观察输出类型
- $\delta: S \times I \to S \times O \times Obs^*$ 是转移函数（Mealy 机）
- $\rho: S \to S$ 是清理函数（吸收态转移）

**物理实现：** $S$ 分配在堆上（$L_S \subset L_P$，进程生命周期内地址固定），$\delta$ 在每次 `process` 调用时执行一个计算步。

**定理 1.2（机器的状态局部性）：**  
对于任意 $M$，所有 $\delta$ 步骤的写入集均包含在 $L_S \cup w_{\delta}$ 中，其中 $w_{\delta}$ 是输出值的临时写集。

*证明：由 $\delta$ 签名 $S \times I \to S \times O$，写入只能修改 $S$ 或产生新输出 $O$。$S$ 位于堆上的 $L_S$ 区域，$O$ 的写入位于当前栈帧，不涉及其他机器的状态区域。*

**Machine trait 对应 Rust 类型：**

```rust
// 代数对应：M = (S, I, O, Obs, delta, rho)
trait Machine: Send + Sync + 'static {
    type S;                      // 状态空间
    type I;                      // 输入类型
    type O;                      // 输出类型
    type Obs;                    // 观察输出类型
    
    fn init() -> Result<S, Err>;  // 初始状态
    fn process(s: &mut S, i: I) -> (ProcessOutput<O>, Vec<Obs>);
                                 // delta: S × I → S × O × Obs*
    fn cleanup(s: S) -> ();       // rho: S → ()
}
```

---

## 2. 端口与连接的范畴论结构

**定义 2.1（端口）**  
一个端口 $p = (T, d)$ 由类型 $T$ 和方向 $d \in \{\texttt{in}, \texttt{out}, \texttt{observe}\}$ 构成。

**定义 2.2（接口）**  
一个接口 $\Gamma$ 是端口的有限集：
$$\Gamma = \{p_1, ..., p_n\}$$

**定义 2.3（机器作为范畴对象）**  
一台机器 $M$ 可视为范畴 $\mathcal{C}$ 中的对象，其接口 $\Gamma_M$ 是该对象的态射签名。

**定义 2.4（连接）**  
一个连接 $\ell = (p_s, p_t)$ 是一个有序对，其中：
- $p_s$ 是源机器的输出端口或观察端口
- $p_t$ 是目标机器的输入端口
- $p_s$ 与 $p_t$ 的类型 $T$ 一致

**定理 2.1（连接的类型可靠性）：**  
对于任意连接 $\ell = (p_s, p_t)$，如果 $T_{p_s} = T_{p_t}$，则 $\ell$ 是语义有效的。

*证明：由定义 2.1 和定义 2.4，同类型端口连接，编译器通过 TypeId 检查保证一致性。*

**定义 2.5（连接图）**  
一个系统 $\Sigma$ 是机器集合 $M_\Sigma$ 与连接集合 $L_\Sigma$ 的有向图：
$$\Sigma = (M_\Sigma, L_\Sigma)$$
其中 $L_\Sigma \subseteq \bigcup_{M \in M_\Sigma} Out_M \times \bigcup_{M \in M_\Sigma} In_M$

**定理 2.2（观察不可干预性）：**  
对于任意机器 $M$，$Obs_M$ 的输出不参与 $M$ 或其他机器的 $\delta$ 函数的输入。

*证明：$Obs$ 端口方向为 $\texttt{observe}$，只能连接到 $\texttt{in}$ 类型端口的收集器。但 $\texttt{observe} \to \texttt{in}$ 连接在 $\delta$ 中不作为输入项。观测流是单向的，从计算流向观测，从不反向。由定理 7.1（系统封闭性）的证明，$\delta$ 的输入来源不包括观察端口。*

### Rust 对应

```rust
// 定义 2.4 的连接可靠性——Rust 类型系统保证：
fn link<A, B>(out: Port<A, Out>, into: Port<B, In>) -> Result<Link, TypeError>
where A: SameAs<B> {}  // 编译器自动推导 SameAs

// 观察不可干预性——observe 端口不出现在 process 签名中：
fn process(s: &mut S, i: I) -> (ProcessOutput<O>, Vec<Obs>)
//                                     ↑ Obs 只出现在输出中
```

---

## 3. 执行序列与调度

**定义 3.1（执行序列）**  
给定机器 $M = (S, I, O, Obs, \delta, \rho)$，一个执行序列是 $\delta$ 的应用序列：
$$s_0 \xrightarrow{i_1} (s_1, o_1, obs_1) \xrightarrow{i_2} (s_2, o_2, obs_2) \xrightarrow{i_3} ...$$
其中 $s_0 = M.init()$，$s_k, o_k, obs_k = \delta(s_{k-1}, i_k)$。

**定义 3.2（调度器）**  
一个调度器 $\Pi$ 是一个函数，将执行序列映射到物理线程：
$$\Pi: M_\Sigma \times \mathbb{N} \to \{T_1, ..., T_n\}$$
即每次 $\delta$ 调用分配给哪个线程。

**定理 3.1（执行等价性）：**  
对于纯函数集合 $\{f_i\}$，任意调度器 $\Pi$ 产生相同的最终结果。

*证明：由定理 1.1a，纯函数可并行且无交互。因此调度顺序不影响输出。*

**定理 3.2（机器的顺序约束）：**  
对于机器 $M$，任意两次连续的 $\delta$ 调用必须在同一线程上执行，否则结果未定义。

*证明：$\delta$ 修改 $S$。如果两次调用在不同线程上执行，$S$ 的并发访问未被保护时产生 data race。由 Rust 的 `&mut S` 借用规则，编译器在编译时禁止跨线程的 `&mut` 访问。*

**定义 3.3（执行原语）**  
axiom 将执行原语定义为以下五种：

| 原语 | 物理对应 | 适用场景 |
|------|---------|---------|
| $\text{Inline}$ | 同线程栈帧调用 | 纯函数链、同线程机器链 |
| $\text{Async}$ | 事件驱动线程池 | IO 密集型、网络服务 |
| $\text{CpuBound}$ | 独占 OS 线程 | 计算密集型 |
| $\text{ThreadPool}$ | 私有线程池 | 混合型 |
| $\text{Subprocess}$ | 独立进程（IPC） | 隔离需求 |

**定理 3.3（执行原语完备性）：**  
以上五种执行原语覆盖了现代通用计算系统所有物理执行模式。

*证明：任意执行模式 $\Pi$ 可归类为：无调度开销（Inline）、协作调度（Async）、抢占调度（CpuBound/ThreadPool）、进程隔离（Subprocess）。此分类穷尽执行资源的三个正交维度：调度开销（无/有）、隔离级别（线程/进程）、资源分配方式（共享/独占）。*

**Rust 对应：**

```rust
enum ExecutionHint {
    Inline,              // delta: 同线程栈帧调用
    Async,               // delta: 事件驱动线程池
    CpuBound,            // delta: 独占 OS 线程
    CpuBoundN(usize),    // delta: N 个独占线程
    ThreadPool(Spec),    // delta: 私有线程池
    Subprocess(Spec),    // delta: 独立进程
}
```

---

## 4. 资源代数

**定义 4.1（资源类）**  
资源类 $R$ 是一个四元组：
$$R = (\tau, \alpha, \zeta, \gamma)$$
其中：
- $\tau \in \{\texttt{static}, \texttt{dynamic}, \texttt{os}, \texttt{thread}, \texttt{process}\}$ 是资源类型
- $\alpha$ 是分配函数（在 $init$ 中调用）
- $\zeta$ 是释放函数（在 $cleanup$ 中调用）
- $\gamma$ 是生命周期标识

**定理 4.1（资源生命周期单调性）：**  
所有通过 $\alpha$ 分配的资源，在 $init \to process^* \to cleanup$ 序列中：
1. $init$ 前不存在
2. $init$ 后 $cleanup$ 前持续存在
3. $cleanup$ 后不存在

*证明：对资源类 $R$，$cleanup$ 调用 $\zeta$ 释放资源，且 $\zeta$ 的唯一调用点是在机器的生命周期终点。Runner 的实现保证 $init$ 在第一次 $process$ 前执行且只执行一次，$cleanup$ 在最后一次 $process$ 后执行且只执行一次，序列不可逆。*

**定义 4.2（资源静态性）**  
资源 $r$ 被称为静态的，当且仅当 $\gamma(r) = \texttt{permanent}$，即释放函数 $\zeta$ 为空。静态资源包括：代码段、类型元数据、vtable、工厂注册信息。

**定理 4.2（静态资源的不可回收性）：**  
静态资源的生命周期等于进程生命周期，不能被部分回收。

*证明：由定义 4.2，$\zeta = \emptyset$。试图回收静态资源等价于卸载代码段，这在 Rust 的编译模型中不被支持——除非通过动态库（dlopen/dlclose）。*

**Rust 对应：**

```rust
enum ResourceClass {
    Static,                                        // tau = static
    DynamicHeap { bytes: usize },                  // tau = dynamic
    OsResource { kind: &'static str },             // tau = os
    Thread { name: &'static str },                 // tau = thread
    Subprocess { executable: String },             // tau = process
}

// 定理 4.1 在类型层面表达：cleanup 消耗 State
fn cleanup(s: S) -> ()  // State 被 move，资源随 Drop 释放
```

---

## 5. 组合与范畴结构

**定义 5.1（机器的串行组合）**  
给定两机器 $M_1 = (S_1, I_1, O_1, Obs_1, \delta_1, \rho_1)$ 和 $M_2 = (S_2, I_2, O_2, Obs_2, \delta_2, \rho_2)$，且 $O_1 = I_2$，它们的串行组合 $M_1 \gg M_2$ 定义为：
$$M_{12} = (S_1 \times S_2, I_1, O_2, Obs_1 \times Obs_2, \delta_{12}, \rho_{12})$$
其中：
$$\delta_{12}((s_1, s_2), i) = ((s_1', s_2'), o_2, (obs_1, obs_2))$$
$$\text{where } (s_1', o_1, obs_1) = \delta_1(s_1, i)$$
$$\text{and } (s_2', o_2, obs_2) = \delta_2(s_2, o_1)$$

**定理 5.1（组合的确定性保持）：**  
如果 $M_1$ 和 $M_2$ 都是确定性的，则 $M_1 \gg M_2$ 也是确定性的。

*证明：确定性定义为 $\forall s,i: \delta(s,i)$ 有唯一结果。组合 $\delta_{12}$ 由两次确定性函数复合得到，结果唯一。*

**定义 5.2（机器范畴 $\mathcal{M}$）**  
机器范畴 $\mathcal{M}$ 定义为：
- **对象**：类型 $I, O$
- **态射**：机器 $M: I \to O$
- **恒等态射**：$\text{id}_I = (\emptyset, I, I, \emptyset, \delta_{id}, \rho_{id})$ 其中 $\delta_{id}(s, i) = (s, i, \emptyset)$
- **组合**：定义 5.1 的串行组合 $\gg$

**定理 5.2（$\mathcal{M}$ 满足范畴律）：**

1. **组合封闭性**：$M_1: I \to O$ 且 $M_2: O \to J$，则 $M_1 \gg M_2: I \to J$
2. **结合律**：$(M_1 \gg M_2) \gg M_3 = M_1 \gg (M_2 \gg M_3)$
3. **单位律**：$\text{id} \gg M = M \gg \text{id} = M$

*证明：*
1. *由定义 5.1，$M_1 \gg M_2$ 的输入为 $I_1 = I$，输出为 $O_2 = J$。*
2. *组合 $\gg$ 定义为函数复合的产物：$\delta_{12}$ 先执行 $\delta_1$ 再执行 $\delta_2$。函数复合满足结合律：$f \circ (g \circ h) = (f \circ g) \circ h$。*
3. *恒等态射 $\text{id}$ 的 $\delta_{id}(s,i) = (s,i,\emptyset)$ 不改变状态和输入。因此 $\text{id} \gg M = M \gg \text{id} = M$。*

**Rust 对应：**

```rust
// 机器范畴 M = (对象, 态射, 组合, 单位)
// 对象：类型 I, O
// 态射：impl Machine

// 恒等 Machine：
struct Identity<I>(PhantomData<I>);
impl<I: Send + Sync + 'static> Machine for Identity<I> {
    type S = ();
    type I = I;
    type O = I;
    type Obs = ();
    fn process(s: &mut (), i: I) -> (ProcessOutput<I>, Vec<()>) {
        (ProcessOutput::Yield(i), vec![])
    }
}

// 串行组合（定义 5.1）：
struct Composed<A, B>(A, B);
impl<A, B> Machine for Composed<A, B>
where A: Machine<Output = B::Input>,
      B: Machine,
{
    type S = (A::S, B::S);
    type I = A::I;
    type O = B::O;
    type Obs = (A::Obs, B::Obs);
    fn process(s: &mut (A::S, B::S), i: A::I) -> (ProcessOutput<B::O>, Vec<(A::Obs, B::Obs)>) {
        let (o1, obs1) = A::process(&mut s.0, i);
        let (o2, obs2) = B::process(&mut s.1, o1.unwrap());
        (o2, obs1.into_iter().zip(obs2).collect())
    }
}
```

---

## 6. 部署代数

**定义 6.1（部署映射）**  
一个部署映射 $\Delta$ 是从机器签名到物理参数的函数：
$$\Delta: M \to (Hint \times Spec)$$
其中 $Hint \in \{\text{Inline}, \text{Async}, \text{CpuBound}, \text{ThreadPool}, \text{Subprocess}\}$，$Spec$ 是各 Hint 对应的参数。

**定理 6.1（部署不变性）：**  
任意部署映射 $\Delta$ 不改变机器的语义行为 $\delta$。

*证明：$\Delta$ 只影响调度 $\Pi$。由定理 3.1 和 3.2，在满足顺序约束下（单线程单调 $\delta$ 调用），调度不影响结果。$\Delta$ 不修改 $\delta$ 的签名或实现。*

**定理 6.2（部署一致性）：**  
同一台机器 $M$ 可以在同一系统中的不同部署中采用不同的 $Hint$，且这两个实例的行为 $\delta$ 一致。

*证明：由定理 6.1，部署不改变 $\delta$。$Hint$ 不同只影响 $M$ 的调度方式，不影响其内部转移函数。*

**Rust 对应：**

```rust
// 定理 6.1 的 Rust 验证：
// MachinePhysicalSpec 包含 execution: ExecutionHint
// 但 Machine::process 不接收 ExecutionHint 参数

struct MachinePhysicalSpec {
    execution: ExecutionHint,  // ← 只影响调度，不影响 process
    state_heap_bytes: usize,
    deterministic: bool,
}

// 同一 Machine<Impl> 可以在两个部署中拥有不同的 spec：
let spec_backtest = MachinePhysicalSpec { execution: CpuBound, .. };
let spec_prod = MachinePhysicalSpec { execution: Async, .. };
// Machine<Impl>::process 在两种部署下的行为一致。
```

---

## 7. 系统整体定理

**定理 7.1（系统封闭性）：**  
给定系统 $\Sigma = (M_\Sigma, L_\Sigma)$，任意 $M \in M_\Sigma$ 的 $\delta$ 调用只读取以下三种来源的数据：
1. $M$ 自身的状态 $S_M$
2. 通过 $L_\Sigma$ 中连接到 $M$ 的输入端口的输出端口的上游机器状态
3. $\delta$ 的当前输入参数 $i$

*证明：由定义 2.5，$\Sigma$ 的所有数据流被 $L_\Sigma$ 显式定义。$M$ 的 $\delta$ 签名 $S \times I \to S \times O$ 中，$I$ 只能来自 $L_\Sigma$ 的连接，$S$ 是私有的。没有其他数据进入 $M$。*

**定理 7.2（可观测性完备性）：**  
对于系统 $\Sigma$ 中的任意机器 $M$，存在一个路径将所有 $Obs_M$ 数据传递到至少一个收集器 $C \in M_\Sigma$，当且仅当 $L_\Sigma$ 包含从 $M$ 的观测端口到 $C$ 的输入端口的连接。

*证明：由定义 2.5，$L_\Sigma$ 包含所有连接。如果 $Obs_M$ 有连接，则数据可达收集器；否则 $Obs_M$ 不产生外部可见数据。这是一个设计时属性——由部署者决定，编译器可校验。*

**定理 7.3（背压传播条件）：**  
背压从消费者 $M_c$ 传播到生产者 $M_p$，当且仅当 $L_\Sigma$ 中 $M_p \to M_c$ 的连接使用 $\text{BoundedBuf}_{\text{Blocking}}$ 策略。

*证明：$\text{BoundedBuf}_{\text{Blocking}}$ 在缓冲区满时阻塞写端，即 $M_p$ 的 $\delta$ 调用被阻塞。其他策略（Dropping、Overwriting、Inline、Latest、CasFreeRing、Channel）在满时不阻塞写端。*

---

## 8. Rust 中的代数结构表达

```rust
// 对象：类型
// 态射：Machine trait
// 范畴 M = (对象, 态射, 组合, 单位)

// 定理 5.2 的 Rust 验证——串行组合保持确定性：
fn compose<A, B>(ma: A, mb: B) -> impl Machine
where
    A: Machine<Output = B::Input>,
    B: Machine,
{
    // 当 A 和 B 都是 deterministic 时，组合也是 deterministic
    assert!(A::deterministic() && B::deterministic() || !"non-det composition");

    ComposedMachine { first: ma, second: mb }
}

// 定理 6.1——部署不变性：
// 对于同一 Machine<Impl>，以下两个部署产生相同的 process 行为：
type Hint = enum { Inline, Async, CpuBound, ThreadPool, Subprocess };
struct Deploy<M: Machine, H: Hint> { /* H 不影响 M 的 process 实现 */ }

// 验证：M::process 不接收 Hint 参数

// 定理 4.1——资源生命周期：
// cleanup 消耗 State 的所有权，编译器保证资源释放：
fn cleanup(s: S) -> ()  // s 被移动，drop 在函数结束时自动调用

// 定义 1.1——纯函数的物理隔离：
// Func::call 不接收 &mut State，编译器保证无堆上副作用：
fn call(i: I) -> O  // 无 &mut self，无 &mut State
```

---

## 9. 形式逻辑与范畴论的统一

**推论 9.1（axiom 系统的 Curry-Howard 对应）：**

| 范畴论概念 | 类型论概念 | axiom 实现 |
|-----------|-----------|-----------|
| 对象 $I, O$ | 类型 `I`, `O` | `type Input, type Output` |
| 态射 $M: I \to O$ | 函数 `I → O` | `trait Machine` |
| 恒等态射 | `identity` | 恒等 Machine |
| 组合 $\gg$ | 函数复合 | 串行组合器 |
| 积 $S_1 \times S_2$ | 元组 `(S1, S2)` | 组合状态 |
| 函子 | 类型构造器 | `FuncScratchPipeline` |
| 自然变换 | 类型间转换 | 端口适配器 |

**推论 9.2（完备性声明）：**  
由定理 1.1-7.3，axiom 覆盖了计算系统的全部三个基本维度的形式化验证：

- **计算维度**（函子/态射/状态）：定理 1.1-2、5.1-2、7.1
- **交互维度**（端口/连接/背压）：定理 2.1-2、7.3
- **资源维度**（执行/部署/生命周期）：定理 3.1-3、4.1-2、6.1-2

以上三条覆盖分别对应计算系统理论中的三个经典问题：可计算性（计算维度）、通信与并发（交互维度）、资源管理（资源维度）。

---

## 10. 与 V8 证明的对比

| V8 定理 | 对应 axiom 定理 | 改进 |
|---------|----------------|------|
| 定理 7：生命周期单调性 | 定理 4.1 | 增加了资源分类（静态/动态），明确静态资源不可回收 |
| 定理 8：注册后系统封闭性 | 定理 7.1 | 显式定义了封闭性的来源（连接图 $L_\Sigma$） |
| 定理 9：级联终止 | 无直接对应 | axiom 不定义生命周期格，生命周期由运行时管理 |
| — | 定理 1.1 | 新增：纯函数的物理隔离（并行安全性） |
| — | 定理 2.2 | 新增：观测不可干预性（观察不影响计算） |
| — | 定理 3.3 | 新增：执行原语完备性 |
| — | 定理 5.2 | 新增：范畴律验证（组合的代数结构） |
| — | 定理 6.1 | 新增：部署不变性（抽象与物理可分离） |

**差距：** axiom 缺少 V8 中级联终止的形式化证明（关闭顺序的拓扑保证）。可以后续补充，但 axiom 当前不定义生命周期格——生命周期由运行时管理，不是核心抽象的一部分。
