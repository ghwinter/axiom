//! 载体（Carrier）——cell_core 因果数据流的物理实现。
//!
//! 回答唯一问题："这条流的值怎么从 `A.out` 到 `B.in`，以何种时空成本"。
//! 每个载体是独立、可替换的物理方案；换载体不改拓扑（多物理实现，T6）。

use axiom::cell_core::PortCell;

/// 载体的时空成本声明——"部署期物理"的量化（非性能承诺，是可选信息）。
///
/// 默认值取最保守的 [`External`](CarrierCost::External)：第三方载体**必须显式声明**
/// 成本，忘写不会被静默当成"零分配"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum CarrierCost {
    /// 零分配、内联（栈上直接传 / 编译期展开）。
    ZeroAllocInline,
    /// 每消息堆分配 + 同步（队列/通道，跨线程）。
    PerMessageAlloc,
    /// 需要分配器或外部机制（共享内存、无锁环等）——保守默认。
    #[default]
    External,
}
// 注：`ZeroAllocInline < PerMessageAlloc < External`（声明序，越小越便宜）；
// `validate_cost` 用 `declared <= budget` 判定：budget 取 `ZeroAllocInline` 时
// 仅零分配载体合格（默认未声明的 External 会被拒绝——保守）。

/// 一个载体：把 cell `A` 的因果输出流动到 cell `B` 的输入。
///
/// 类型层保证 `A: PortCell, B: PortCell<In = A::Out>`（即这条因果流本身合法，T1）。
/// `flow` 是物理实现——它直接调用 `A::step`/`B::step`，或用通道/其他机制传递。
pub trait Carrier<A, B>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    /// 本载体的时空成本声明（默认保守为 [`External`](CarrierCost::External)；
    /// 实现者应显式声明真实成本）。
    fn cost() -> CarrierCost {
        CarrierCost::External
    }

    /// 把 `A` 的一个输入流经 `A`，再经本载体流入 `B`，返回 `B` 的输出。
    ///
    /// `flow(state_a, state_b, input) -> B::Out`
    /// 实现自由选择物理方案（内联 / 队列 / 编译期展开），但语义上等价于
    /// `B::step(sb, A::step(sa, input))`（因果数据流）。
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out;
}

/// 栈上函数直接传载体：`A::step` 的结果直接作 `B::step` 的输入。
///
/// 零分配、内联，编译后等价手写 `B::step(sb, A::step(sa, x))`（T7 静态路径）。
/// 单线程同步语义；无运行时对象。
pub struct InlineCarrier;

impl<A, B> Carrier<A, B> for InlineCarrier
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    fn cost() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }

    #[inline(always)]
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        let mid = A::step(sa, input);
        B::step(sb, mid)
    }
}

/// 队列载体：把 `A` 的输出经一个 FIFO 队列中转后流入 `B`。
///
/// 每次传输 = 一次 `Box<dyn Any>` 堆分配（类型擦除的物理代价）+ 队列 push/pop。
/// 体现"不同时空成本 = 不同载体"：总线放（Inline）→ 零分配；队列中转 → 每消息分配。
/// 真正的跨线程调度是运行时驱动（`flow` 模块）的职责，本载体聚焦"队列中转"的物理形态。
///
/// 需要 `std`（`Box<dyn Any>` downcast）。`no_std` 构建下仅有 Inline 零分配载体
/// （Direct 已并入 Inline；跨线程经 [`spawned_flow`]）。
#[cfg(feature = "std")]
pub struct QueueCarrier;

#[cfg(feature = "std")]
impl<A, B> Carrier<A, B> for QueueCarrier
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    A::Out: core::any::Any + Send + 'static,
{
    fn cost() -> CarrierCost {
        CarrierCost::PerMessageAlloc
    }

    #[inline(always)]
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        // ① A 产出输出。
        let mid = A::step(sa, input);
        // ② 经队列中转：装箱（每消息堆分配）→ 立即出队解包。
        //   （演示"队列/类型擦除"的物理形态；真实异步队列由 flow 驱动管理。）
        let boxed: Box<dyn core::any::Any + Send> = Box::new(mid);
        let unboxed: Box<A::Out> = boxed
            .downcast::<A::Out>()
            .expect("QueueCarrier 类型应匹配 A::Out");
        // ③ 流入 B。
        B::step(sb, *unboxed)
    }
}

/// 有界/背压载体：把 `A` 的输出经一个**有界** FIFO（容量 `CAP`）中转。
///
/// 这是 §9.1"有界/背压"的**载体侧**：容量上限 `CAP` 是编译期常量，物理形态为有界通道/队列。
/// **编译期门**：`CAP >= 1` 由 [`assert_capacity_nonzero`](crate::contract::assert_capacity_nonzero)
/// 强制（`CAP = 0` 是 rendezvous：同线程 `send` 先于 `recv` 会永久死锁）；真正的多消息
/// **阻塞背压**由 [`bounded_pump`](crate::flow::bounded_pump)（生产端满时阻塞）与
/// [`BoundedQueue`](crate::buffer::BoundedQueue)（`try_push` 满返回容量信号）承载。
///
/// 需要 `std`（有界 `sync_channel`）。安全。
#[cfg(feature = "std")]
pub struct BoundedCarrier<const CAP: usize>;

#[cfg(feature = "std")]
impl<A, B, const CAP: usize> Carrier<A, B> for BoundedCarrier<CAP>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    A::Out: Send + 'static,
{
    fn cost() -> CarrierCost {
        CarrierCost::PerMessageAlloc
    }

    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        // 编译期门（模态②）：CAP >= 1，拒绝 rendezvous 死锁形态（语句式 const 块，编译期求值）。
        const { crate::contract::assert_capacity_nonzero::<CAP>() };
        // 经容量 CAP 的有界通道中转：体现"容量上限由编译期常量决定"的物理形态。
        let (tx, rx) = std::sync::mpsc::sync_channel::<A::Out>(CAP);
        let mid = A::step(sa, input);
        let _ = tx.send(mid);
        let v = rx.recv().expect("bounded channel alive");
        B::step(sb, v)
    }
}

/// 跨线程执行 `B::step`：`A` 在调用线程产出输出，经 mpsc 投到专用线程，
/// 该线程持有 `B::State` 并执行 `B::step`，输出经另一 mpsc 传回调用线程。
///
/// 这是"通道/堆队列、跨线程"物理方案的直接体现——`B` 的状态被保护在专用线程中，
/// 与调用线程隔离；换此物理形态即把一条因果流移到独立线程上执行（T6 多物理实现）。
///
/// **无 `ChannelCarrier` 类型的原因**：`Carrier::flow` 签名是 `&mut B::State`
/// （无法跨线程借用），故跨线程形态由本独立入口提供——"线程是物理层"（T9/T3）。
/// 真正需要常驻工作线程/流式 worker 时，可在载体目录扩展本形态。
///
/// **终止性保证（panic 传播）**：若工作线程内 `B::step` panic，panic 载荷经回执通道
/// 传回，调用线程在 `recv` 后立即 `resume_unwind`——不会永久阻塞、panic 不丢失。
#[cfg(feature = "std")]
pub fn spawned_flow<A, B>(
    sa: &mut A::State,
    init_b: impl FnOnce() -> B::State + Send + 'static,
    input: A::In,
) -> B::Out
where
    A: PortCell,
    B: PortCell<In = A::Out> + Send + 'static,
    A::In: Send + 'static,
    A::Out: Send + 'static,
    B::Out: Send + 'static,
{
    use std::sync::mpsc;

    /// 工作线程回执：正常输出，或捕获的 panic 载荷。
    enum Reply<O> {
        Out(O),
        Panic(Box<dyn std::any::Any + Send + 'static>),
    }

    // ① A 在调用线程产出输出。
    let mid = A::step(sa, input);

    // ② 建两条通道：调用线程 → 工作线程（输入），工作线程 → 调用线程（回执）。
    let (tx, rx) = mpsc::channel::<A::Out>();
    let (reply_tx, reply_rx) = mpsc::channel::<Reply<B::Out>>();

    // ③ 工作线程持有 B::State，收到输入即执行 B::step；panic 被捕获并回传。
    let worker = std::thread::spawn(move || {
        let mut sb = init_b();
        let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let v = rx.recv().expect("input channel alive");
            B::step(&mut sb, v)
        }));
        let _ = match reply {
            Ok(out) => reply_tx.send(Reply::Out(out)),
            Err(payload) => reply_tx.send(Reply::Panic(payload)),
        };
    });

    // ④ 投一条输入并取回执；若工作线程 panicked，立即在原线程续抛（不挂死、不吞错）。
    tx.send(mid).expect("worker alive");
    let reply = reply_rx.recv().expect("worker replied");
    worker.join().ok();
    match reply {
        Reply::Out(out) => out,
        Reply::Panic(payload) => std::panic::resume_unwind(payload),
    }
}