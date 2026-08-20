//! 驱动：把蓝图（cell 拓扑）+ 载体选型兑现为执行。
//!
//! 这里"兑现"沿 cell_core 的"蓝图即类型"——给定一个 `PortCell` 拓扑和载体，
//! 提供便捷的驱动入口。不同载体 = 不同物理实现（T6）。

use axiom::cell_core::{DoesWire, PortCell};
use crate::carrier::Carrier;

/// 用载体 `C` 驱动一条 A→B 因果流，返回 B 的输出。
///
/// 载体 `C` 决定物理实现（Inline=零分配内联 / Queue=队列中转 / Direct=编译期展开）。
/// 在驱动前做编译期布线验证（`DoesWire`，失败即编译错误）——验证在编译期，运行期零开销。
pub fn drive_link<A, B, C>(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    // 编译期布线判定：A.out 可布到 B.in（DoesWire 对 () 实现）。
    let _: bool = <() as DoesWire<A, B>>::WIRES;
    C::flow(sa, sb, input)
}

/// 驱动一条已验证布线的 A→B 流（显式以 `LINK` 作为布线持证者）。
///
/// `LINK` 须满足 `DoesWire<A, B>`，作为"这条因果流合法"的编译期见证。
pub fn drive_wired<A, B, LINK, C>(
    sa: &mut A::State,
    sb: &mut B::State,
    input: A::In,
) -> B::Out
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    LINK: DoesWire<A, B>,
    C: Carrier<A, B>,
{
    let _ = <() as DoesWire<A, B>>::WIRES;
    let _ = core::marker::PhantomData::<LINK>;
    C::flow(sa, sb, input)
}

/// Result 短路：把"产出 `Result`"的 `A` 连接到一个消费其 Ok 的 `B`，遇 `Err` 立即短路。
///
/// 闭合"错误 / 失败通路"（`foundations.md` §9.2）在**物理侧**的实现：用 `Out = Result`
/// 约定 + 短路。`A::step` 若返回 `Err(e)`，整体短路返回 `Err`（不流入 `B`）；`Ok(mid)`
/// 才经 `B::step` 继续。`X` 是从 `A` 的 `Ok` 到 `B::In` 的中继类型。纯、`no_std` 安全。
pub fn drive_try<A, B, X, E>(
    sa: &mut A::State,
    sb: &mut B::State,
    input: A::In,
) -> Result<B::Out, E>
where
    A: PortCell<Out = Result<X, E>>,
    B: PortCell<In = X>,
{
    let mid: X = A::step(sa, input)?;
    Ok(B::step(sb, mid))
}

/// 可失败链：把两个会失败的 cell（`Out = Result`）串成**短路链**，产出**单层** `Result`。
///
/// 比 `drive_try` 的"`B::Out` 再嵌套"更干净：`A` 的 `Ok(X)` 流入 `B`，任一 `Err` 短路，
/// `Out = Result<B::Ok, E>`（单层）。它是"错误/短路通路"（§9.2）的**可组合一等构造**
/// （一个 `PortCell`）——整条 fallible 流水线可作为一个 cell 复用/组合。
pub struct TryChain<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B, X, Y, E> PortCell for TryChain<A, B>
where
    A: PortCell<Out = Result<X, E>>,
    B: PortCell<In = X, Out = Result<Y, E>>,
{
    type In = A::In;
    type Out = Result<Y, E>;
    type State = (A::State, B::State);

    #[inline(always)]
    fn step((sa, sb): &mut (A::State, B::State), input: A::In) -> Result<Y, E> {
        let x: X = A::step(sa, input)?;
        B::step(sb, x)
    }
}

/// 有界泵：把一组输入，经一个**有界**队列（容量 `CAP`）投到消费者 cell，返回其输出序列。
///
/// 这是 §9.1"有界/背压"的**真实背压**演示：生产端把 `A` 的输出投入容量 `CAP` 的有界
/// `sync_channel`；**当队列满时，生产端阻塞（背压）**，直到消费者线程 drain 腾出空间。
/// 返回值 = 消费者 `B::step` 的输出序列。std 门控、安全。
#[cfg(feature = "std")]
pub fn bounded_pump<A, B, It, const CAP: usize>(
    init_a: impl FnOnce() -> A::State + Send + 'static,
    init_b: impl FnOnce() -> B::State + Send + 'static,
    inputs: It,
) -> Vec<B::Out>
where
    A: PortCell + Send + 'static,
    B: PortCell<In = A::Out> + Send + 'static,
    A::In: Send + 'static,
    A::Out: Send + 'static,
    B::Out: Send + 'static,
    It: IntoIterator<Item = A::In> + Send + 'static,
{
    use std::sync::mpsc;

    let (tx, rx) = mpsc::sync_channel::<A::Out>(CAP);

    // 消费者线程：drain 有界队列，逐条跑 B::step。
    let consumer = std::thread::spawn(move || {
        let mut sb = init_b();
        let mut outs = Vec::new();
        while let Ok(v) = rx.recv() {
            outs.push(B::step(&mut sb, v));
        }
        outs
    });

    // 生产端：跑 A::step，把输出投入有界队列（满则阻塞 = 背压）。
    let mut sa = init_a();
    for input in inputs {
        let mid = A::step(&mut sa, input);
        let _ = tx.send(mid);
    }
    drop(tx); // 关闭通道，令消费者收尾。

    consumer.join().expect("consumer finished")
}

/// 有界泵（可失败生产端）：`失败 × 背压` 联合语义的可测试表达。
///
/// 生产端 `A` 的 `Out = Result<X, E>`：`Ok(x)` 投入容量 `CAP` 的有界队列（满则阻塞 = **背压**）；
/// `Err` **短路**——不投队列、记录错误计数。消费者线程 drain 队列跑 `B::step`。
/// 返回 `(输出序列, 错误计数)`。这演示：在有界（背压）的同时，错误路径不污染队列、不被静默吞掉。
/// std 门控、安全。
#[cfg(feature = "std")]
pub fn bounded_pump_try<A, B, X, E, It, const CAP: usize>(
    init_a: impl FnOnce() -> A::State + Send + 'static,
    init_b: impl FnOnce() -> B::State + Send + 'static,
    inputs: It,
) -> (Vec<B::Out>, usize)
where
    A: PortCell<Out = Result<X, E>> + Send + 'static,
    B: PortCell<In = X> + Send + 'static,
    A::In: Send + 'static,
    X: Send + 'static,
    E: Send + 'static,
    B::Out: Send + 'static,
    It: IntoIterator<Item = A::In> + Send + 'static,
{
    use std::sync::mpsc;

    let (tx, rx) = mpsc::sync_channel::<X>(CAP);
    let consumer = std::thread::spawn(move || {
        let mut sb = init_b();
        let mut outs = Vec::new();
        while let Ok(v) = rx.recv() {
            outs.push(B::step(&mut sb, v));
        }
        outs
    });

    let mut sa = init_a();
    let mut errs = 0usize;
    for input in inputs {
        match A::step(&mut sa, input) {
            Ok(x) => {
                let _ = tx.send(x); // 满则阻塞 = 背压
            }
            Err(_) => errs += 1, // 短路：不投队列、记录错误
        }
    }
    drop(tx);

    let outs = consumer.join().expect("consumer finished");
    (outs, errs)
}

/// 运行期序列驱动：把一组输入依次流经同一个 cell `C`，收集输出。
///
/// 这是"无界计数"的**生成/物理侧**：计数的多少由运行期序列（`IntoIterator`）决定，
/// 而非编译期常量——作为 [`Rep<N,C>`](axiom::cell_core::Rep)（编译期定数）的运行期
/// 对应物；二者统一于"对同型 cell 的反复作用"。状态在序列各次驱动间保持。
#[cfg(feature = "std")]
pub fn drive_seq<C, I, O, It>(state: &mut C::State, inputs: It) -> Vec<O>
where
    C: PortCell<In = I, Out = O>,
    It: IntoIterator<Item = I>,
{
    inputs.into_iter().map(|x| C::step(state, x)).collect()
}
