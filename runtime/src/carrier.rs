//! 载体（Carrier）——cell_core 因果数据流的物理实现。
//!
//! 回答唯一问题："这条流的值怎么从 `A.out` 到 `B.in`，以何种时空成本"。
//! 每个载体是独立、可替换的物理方案；换载体不改拓扑（多物理实现，T6）。

use axiom::cell_core::PortCell;

/// 载体的时空成本声明——"部署期物理"的量化（非性能承诺，是可选信息）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CarrierCost {
    /// 零分配、内联（栈上直接传 / 编译期展开）。
    #[default]
    ZeroAllocInline,
    /// 每消息堆分配 + 同步（队列/通道，跨线程）。
    PerMessageAlloc,
    /// 需要分配器或外部机制（共享内存、无锁环等）。
    External,
}

/// 一个载体：把 cell `A` 的因果输出流动到 cell `B` 的输入。
///
/// 类型层保证 `A: PortCell, B: PortCell<In = A::Out>`（即这条因果流本身合法，T1）。
/// `flow` 是物理实现——它直接调用 `A::step`/`B::step`，或用通道/其他机制传递。
pub trait Carrier<A, B>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    /// 本载体的时空成本声明。
    fn cost() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }

    /// 把 `A` 的一个输入流经 `A`，再经本载体流入 `B`，返回 `B` 的输出。
    ///
    /// `flow(state_a, state_b, input) -> B::Out`
    /// 实现自由选择物理方案（内联 / 队列 / 编译期展开），但语义上等价于
    /// `B::step(sb, A::step(sa, input))`（因果数据流）。
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out;
}

// ═══════════════════════════════════════════════════════════════════════════
// InlineCarrier —— 栈上函数直接传，零分配、内联、单线程
// ═══════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════
// QueueCarrier —— 值经队列中转（每消息分配的物理形态）
// ═══════════════════════════════════════════════════════════════════════════

/// 队列载体：把 `A` 的输出经一个 FIFO 队列中转后流入 `B`。
///
/// 每次传输 = 一次 `Box<dyn Any>` 堆分配（类型擦除的物理代价）+ 队列 push/pop。
/// 体现"不同时空成本 = 不同载体"：总线放（Inline）→ 零分配；队列中转 → 每消息分配。
/// 真正的跨线程调度是运行时驱动（`flow` 模块）的职责，本载体聚焦"队列中转"的物理形态。
///
/// 需要 `std`（`Box<dyn Any>` downcast）。`no_std` 构建下仅有 Inline/Direct 零分配载体。
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

// ═══════════════════════════════════════════════════════════════════════════
// DirectCarrier —— 编译期展开标记（静态链内联为调用图，零运行时对象）
// ═══════════════════════════════════════════════════════════════════════════

/// 编译期展开载体（标记）：把静态链（`Static<Chain<...>>`）在编译期内联展开，
/// 无运行时对象、零分配。实现依赖 cell_core 的 `Static`——驱动在 `flow` 模块。
pub struct DirectCarrier;

impl<A, B> Carrier<A, B> for DirectCarrier
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    fn cost() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }

    #[inline(always)]
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        // 与 InlineCarrier 同构（编译期展开 = 内联直接传）；体现"编译期折叠"。
        let mid = A::step(sa, input);
        B::step(sb, mid)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ChannelCarrier（std）—— 真正的跨线程通道载体
// ═══════════════════════════════════════════════════════════════════════════

/// 跨线程通道载体（`std`）：把 `A` 的输出经 `mpsc` 投到一个**独立线程**上，
/// 由持有 `B::State` 的工作线程执行 `B::step`，结果经 `oneshot` 送回调用线程。
///
/// 这是"堆队列/通道、跨线程"作为可替换物理方案（对应未来 axiom-tokio 的异步载体）。
/// 因 `Carrier::flow` 签名是 `&mut B::State`（无法跨线程借用），跨线程形态由
/// [`spawned_flow`](fn@spawned_flow) 提供独立入口——它把 `B::State` 移入工作线程，
/// 体现"线程是物理层"（T9/T3）。
#[cfg(feature = "std")]
pub struct ChannelCarrier;

/// 跨线程执行 `B::step`：`A` 在调用线程产出输出，经 mpsc 投到专用线程，
/// 该线程持有 `B::State` 并执行 `B::step`，输出经另一 mpsc 传回调用线程。
///
/// 这是"通道/堆队列、跨线程"物理方案的直接证明——`B` 的状态被保护在专用线程中，
/// 与调用线程隔离；换此载体即把一条因果流移到独立线程上执行（T6 多物理实现）。
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

    // ① A 在调用线程产出输出。
    let mid = A::step(sa, input);

    // ② 建两条通道：调用线程 → 工作线程（输入），工作线程 → 调用线程（输出）。
    let (tx, rx) = mpsc::channel::<A::Out>();
    let (reply_tx, reply_rx) = mpsc::channel::<B::Out>();

    // ③ 工作线程持有 B::State，收到输入即执行 B::step，结果回传。
    let worker = std::thread::spawn(move || {
        let mut sb = init_b();
        match rx.recv() {
            Ok(v) => {
                let out = B::step(&mut sb, v);
                let _ = reply_tx.send(out);
            }
            Err(_) => {}
        }
    });

    // ④ 投一条输入并取结果。
    tx.send(mid).expect("worker alive");
    let out = reply_rx.recv().expect("worker replied");
    worker.join().expect("worker finished");
    out
}

