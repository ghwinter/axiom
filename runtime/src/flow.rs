//! 驱动：把蓝图（cell 拓扑）+ 载体选型兑现为执行。
//!
//! 这里"兑现"沿 cell_core 的"蓝图即类型"——给定一个 `PortCell` 拓扑和载体，
//! 提供便捷的驱动入口。不同载体 = 不同物理实现（T6）。

use alloc::vec::Vec;
use axiom::cell_core::{Conforms, PortCell, Wire};
use crate::carrier::Carrier;

/// 用载体 `C` 驱动一条 A→B 因果流，返回 B 的输出。
///
/// 载体 `C` 决定物理实现（Inline=零分配内联 / Queue=队列中转 / Bounded=有界通道）。
/// 在驱动前做编译期布线验证（`Conforms<Wire<A,B>>`，失败即编译错误）——验证在编译期，运行期零开销。
pub fn drive_link<A, B, C>(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    // 编译期布线判定：A.out 可布到 B.in（对偶组合 T1，统一 Conforms 判据）。
    let _: bool = <() as Conforms<Wire<A, B>>>::OK;
    C::flow(sa, sb, input)
}

/// 无缓冲内联闭环驱动：`BODY -> FEED -> BODY` 经内联载体一拍（一次调用内两拍）。
///
/// **门禁**：要求 `FEED:` [`Moore`](crate::contract::Moore)（部署者声明，模态④——仅声明、
/// 非证明：输出是否真的只依赖 `State` 是语义性质、不可判定（Rice），错误声明由声明者负责）。
/// 有缓冲环（`BoundedCarrier`/队列）无需此门——缓冲即延迟（T3）。
///
/// > 与核心 [`Feedback`](axiom::cell_core::Feedback)`::step` 的关系**待裁定**：核心的单元
/// > 形式亦固定"一拍两算"且不要求 `Moore`；本入口是 runtime 侧"门禁落位"的驱动，二者语义
/// > 一致性由审计项 S7 跟踪。
pub fn drive_feedback_inline<BODY, FEED>(
    sb: &mut BODY::State,
    sf: &mut FEED::State,
    external: BODY::In,
) -> BODY::Out
where
    BODY: PortCell,
    FEED: PortCell<In = BODY::Out, Out = BODY::In> + crate::contract::Moore,
{
    let out = BODY::step(sb, external);
    let next_in: BODY::In = FEED::step(sf, out);
    BODY::step(sb, next_in)
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
/// 与 `drive_try` 的差异：`drive_try` 只要求 `A` 产生 `Result`、`B` 可**非失败**
/// （"OK 通道"专线，`Out = Result<B::Out, E>`）；`TryChain` 要求 `A` 与 `B` **都**产生
/// `Result` 且共享同一错误类型 `E`，产出单层 `Out = Result<Y, E>`，本身是 `PortCell`
/// （可再组合）——整条 fallible 流水线可作为一个 cell 复用。
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
///
/// **拆除语义**：若消费者线程终止（`B::step` panic 使 `rx` 断连），`send` 失败——
/// 生产端立即停止投递（不再空转生产），随后由 `consumer.join()` 显式暴露 panic；
/// 拆除时未投递的值随管道一起丢弃，不静默延续生产。
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

    const { crate::contract::assert_capacity_nonzero::<CAP>() };
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
        if tx.send(mid).is_err() {
            break; // 消费者已终止（断连）：停止生产，交由 join 暴露（拆除语义，见函数文档）。
        }
    }
    drop(tx); // 关闭通道，令消费者收尾。

    consumer.join().unwrap_or_else(|p| std::panic::resume_unwind(p))
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

    const { crate::contract::assert_capacity_nonzero::<CAP>() };
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
                if tx.send(x).is_err() {
                    break; // 消费者已终止（断连）：停止生产，交由 join 暴露（拆除语义，同 bounded_pump）。
                } // 满则阻塞 = 背压
            }
            Err(_) => errs += 1, // 短路：不投队列、记录错误
        }
    }
    drop(tx);

    let outs = consumer.join().unwrap_or_else(|p| std::panic::resume_unwind(p));
    (outs, errs)
}

/// 运行期序列驱动：把一组输入依次流经同一个 cell `C`，收集输出。
///
/// 这是"无界计数"的**生成/物理侧**：计数的多少由运行期序列（`IntoIterator`）决定，
/// 而非编译期常量——作为 [`Rep<N,C>`](axiom::cell_core::Rep)（编译期定数）的运行期
/// 对应物；二者统一于"对同型 cell 的反复作用"。状态在序列各次驱动间保持。
///
/// 仅用 `alloc`（`Vec`），不依赖 `std`——`no_std + alloc` 下亦可作为"无界计数"激活入口。
/// 序列物化为 `Vec` 是本入口声明的生成成本；零成本单步路径在核心 `drive`。
pub fn drive_seq<C, I, O, It>(state: &mut C::State, inputs: It) -> Vec<O>
where
    C: PortCell<In = I, Out = O>,
    It: IntoIterator<Item = I>,
{
    inputs.into_iter().map(|x| C::step(state, x)).collect()
}
