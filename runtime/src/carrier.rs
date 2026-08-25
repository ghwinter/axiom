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

    /// 本载体接缝的义务类声明（C10 分化）。默认**保守 fail-closed**（资源=External）；
    /// 每个实现者应覆写：resource 取 [`Self::cost`] 同值，有投递语义者再补 delivery 轴。
    fn obligation() -> crate::obligation::ObligationClass {
        crate::obligation::ObligationClass::default()
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

    fn obligation() -> crate::obligation::ObligationClass {
        crate::obligation::ObligationClass {
            resource: CarrierCost::ZeroAllocInline,
            ..crate::obligation::ObligationClass::default()
        }
    }

    #[inline(always)]
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        let mid = A::step(sa, input);
        B::step(sb, mid)
    }
}

/// 队列载体（类型擦除传递的**演示形态**）：把 `A` 的输出经一次装箱/解包流入 `B`。
///
/// 每次传递 = 一次 `Box<dyn Any>` 堆分配 + downcast（类型擦除的物理代价）。
/// **本形态不是真实 FIFO 缓冲**：装箱立即解包，无跨步延迟/重排——它演示的是
/// "队列/类型擦除"的成本形态（每消息分配）。真实的有界阻塞背压由 [`BoundedCarrier`]
/// 与 [`bounded_pump`](crate::flow::bounded_pump) 承载；跨线程调度由 [`spawned_flow`] 承载。
///
/// 需要 `std`（`Box<dyn Any>` downcast）。`no_std` 构建下仅有 Inline 零分配载体
/// （Direct 已并入 Inline）。
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

    fn obligation() -> crate::obligation::ObligationClass {
        crate::obligation::ObligationClass {
            delivery: crate::obligation::DeliveryKind::MechanizedFullClosed,
            resource: CarrierCost::PerMessageAlloc,
            ..crate::obligation::ObligationClass::default()
        }
    }

    #[inline(always)]
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        // ① A 产出输出。
        let mid = A::step(sa, input);
        // ② 类型擦除传递：装箱（每消息堆分配）→ 立即解包（**非真实 FIFO 缓冲**）。
        //   （演示"队列/类型擦除"的成本形态；真实有界背压见 BoundedCarrier/bounded_pump。）
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

    fn obligation() -> crate::obligation::ObligationClass {
        crate::obligation::ObligationClass {
            delivery: crate::obligation::DeliveryKind::MechanizedFullClosed,
            resource: CarrierCost::PerMessageAlloc,
            ..crate::obligation::ObligationClass::default()
        }
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
/// **终止性保证（panic 传播）**：若工作线程内 `init_b` 或 `B::step` panic，panic 载荷经回执
/// 通道传回，调用线程立即 `resume_unwind`——不会永久阻塞、panics 不丢失、原载荷保留。
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
        let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut sb = init_b();
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

// ═══════════════════════════════════════════════════════════════════
// 一等短路载体（§9.2 收账；失败为值经"载体形态"串接）
// ═══════════════════════════════════════════════════════════════════

/// 短路链路能力：生产端 `A` 的 `Out = Result<X, E>`；经本载体把 `Ok(x)` 送入消费端
/// `B`（`In = X`），`Err` 短路返回（`B` 不被执行）。
///
/// **诚实说明（A5）**：标准 [`Carrier`] 的界 `B::In = A::Out` 无法表达 X-lane
/// （`A::Out = Result<X,E>` 而 `B::In = X`），故短路以**一等能力**形态落地，不改动
/// `Carrier` trait（T6 契约不变）；与组合子 [`TryChain`](crate::flow::TryChain)/
/// [`drive_try`](crate::flow::drive_try) 同语义、不同物理表达。§9.2 余项由此收账。
pub trait ShortCircuit<A, B, X, E>
where
    A: PortCell,
    B: PortCell,
{
    /// 驱动一条短路链：`Ok` 直通 `B`，`Err` 短路（`step` 保持全函数，错误为值）。
    fn run(sa: &mut A::State, sb: &mut B::State, input: A::In) -> Result<B::Out, E>;
}

/// `Result` 短路载体：`Ok` 直通、`Err` 短路。
pub struct ResultCarrier;

impl<A, B, X, E> ShortCircuit<A, B, X, E> for ResultCarrier
where
    A: PortCell<Out = Result<X, E>>,
    B: PortCell<In = X>,
{
    #[inline(always)]
    fn run(sa: &mut A::State, sb: &mut B::State, input: A::In) -> Result<B::Out, E> {
        match A::step(sa, input) {
            Ok(x) => Ok(B::step(sb, x)),
            Err(e) => Err(e),
        }
    }
}

/// `Maybe` 短路载体：与 `ResultCarrier` 同机制（`Option` 语域是 `E = ()` 的特例）；
/// 命名区分投递语域而非机制。同为实现 [`ShortCircuit`] 的零尺寸令牌。
pub struct MaybeCarrier;

impl<A, B, X, E> ShortCircuit<A, B, X, E> for MaybeCarrier
where
    A: PortCell<Out = Result<X, E>>,
    B: PortCell<In = X>,
{
    #[inline(always)]
    fn run(sa: &mut A::State, sb: &mut B::State, input: A::In) -> Result<B::Out, E> {
        match A::step(sa, input) {
            Ok(x) => Ok(B::step(sb, x)),
            Err(e) => Err(e),
        }
    }
}

/// 经短路载体驱动（一等短路载体的装配入口；§9.2）。
#[inline(always)]
pub fn drive_try_carrier<C, A, B, X, E>(
    sa: &mut A::State,
    sb: &mut B::State,
    input: A::In,
) -> Result<B::Out, E>
where
    C: ShortCircuit<A, B, X, E>,
    A: PortCell,
    B: PortCell,
{
    C::run(sa, sb, input)
}

#[cfg(test)]
mod short_circuit_tests {
    use super::*;

    struct Fallible;
    #[derive(Debug, PartialEq)]
    struct Fail;
    impl PortCell for Fallible {
        type In = i32;
        type Out = Result<i32, Fail>;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> Result<i32, Fail> {
            if x < 0 { Err(Fail) } else { Ok(x + 1) }
        }
    }
    struct Double;
    impl PortCell for Double {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x * 2
        }
    }

    #[test]
    fn short_circuit_ok_passes_and_err_skips() {
        let (mut sa, mut sb) = ((), ());
        assert_eq!(
            drive_try_carrier::<ResultCarrier, Fallible, Double, _, _>(&mut sa, &mut sb, 5),
            Ok(12) // Ok(6) -> Double 12
        );
        assert_eq!(
            drive_try_carrier::<ResultCarrier, Fallible, Double, _, _>(&mut sa, &mut sb, -1),
            Err(Fail) // 短路:B 不执行
        );
        assert_eq!(
            drive_try_carrier::<MaybeCarrier, Fallible, Double, _, _>(&mut sa, &mut sb, 7),
            Ok(16) // Ok(8) -> Double 16
        );
    }
}