//! 型位的运行期存在化（∃ 绑定，物理侧）——许可生命周期 typestate 形态（模态①）。
//!
//! 型位 `Slot<I,O>` 在 core 由 `Conforms` 编译期验证（T1：任何 `In=I, Out=O` 的居留项
//! 都合规）。本模块把"运行时**选择**一个合规居留项并**驱动**"做成安全的类型擦除：
//! 居留项的状态类型擦除为 `Box<dyn Any + Send>`，`step` 以函数指针保存
//! （针对该居留项单态化）。这是统一模型里"∃ 绑定"的**物理侧**——接口固定
//! （T1 编译期验证）、居留项运行期存在化、可换装。
//!
//! 生命周期（meta-foundations 定义 1.6 的生命周期轴；runtime-constitution 阶段 2）：
//! - **Adding**：`SlotPending::install::<T>(state)`——安装一个编译期合规居留项
//!   （`T: PortCell<In=I, Out=O>` ⟹ core `Conforms` 判定；T1 编译期验证）；
//! - **Ready → Live**：`commit()`——授权；此后才可 `drive`。未 commit 不可驱动是
//!   **类型级**拒绝（模态① 结构见证，零运行期检查）——落位律 A3 对"许可何时生效"
//!   的最强模态放置；
//! - **Finished**：`SlotDrive::drive` / [`seat`](SlotDrive::seat) / `swap`
//!   （`SlotDrive` 沿用规范概念名：存在绑定（existential binding）即"已被授权驱动的实体"）；
//! - **Cleaned**：`retire()` 或 drop。
//!
//! 代戳（引用有效轴）：[`Seat`] 携带创建时的代；`swap` 递增代；`drive` 校验代一致，
//! 不一致即拒绝（陈旧引用契约）。
//!
//! **诚实声明（A5）**：当前形态下 `Seat` 独占 `&mut SlotDrive`，陈旧借用已被借用检查器在
//! 安全代码中排除（模态①）；代戳是为未来内部共享变体（如 `Arc<Mutex<..>>` 驱动的间接
//! 形态）保留的机制契约，本形式以测试锁定其代递增与校验行为。
//!
//! **在途值归属（C5；热替换语义）**：换装可能丢弃旧居留项状态中**未完成的工作**
//! （在途值）。静默丢弃违反 A5——本模块提供两种显式形态：
//! - [`SlotDrive::swap`]：已知无在途的换装（调用方断言旧实现无未完成工作；
//!   违反 = 声明者责任）；
//! - [`SlotDrive::swap_and_drain`]：**强制处置**旧状态——旧状态连同其未完成工作
//!   交给调用方回调，由调用方裁定归属（排空回传 / 随 `Closed` 回传 / 重建）。
//!   静默丢弃在**类型上不可能**（回调必须存在）。
//! 居留项可实现 [`Drainable`] 声明"可能持有在途"，换装时以 [`Drainable::drain_pending`]
//! 把工作余量以值交出。并发共享变体（`Arc<Mutex<..>>`）的 quiesce 协议待该形态落地
//! （衔接 async-seam.md 开放问题：异步接缝 × 换装的在途语义）。
//!
//! **成本声明（模态③）**：每次安装/换装一次堆分配（`Box`）+ 函数指针间接调用——
//! 本接缝是 runtime 的动态税位置之一（PerInstallAlloc 类），部署期显式声明。

use axiom::cell_core::PortCell;
use core::any::Any;

type Step<I, O> = fn(&mut Box<dyn Any + Send>, I) -> O;

/// 安装阶段（Adding）：已安装居留项，**未授权**。无 `drive` 方法（模态①，类型级拒绝）。
#[cfg(feature = "std")]
pub struct SlotPending<I, O> {
    state: Box<dyn Any + Send>,
    step: Step<I, O>,
}

/// 已授权驱动（Ready/Finished）：存在绑定的运行期实体（规范概念名）。
#[cfg(feature = "std")]
pub struct SlotDrive<I, O> {
    state: Box<dyn Any + Send>,
    step: Step<I, O>,
    generation: u64,
}

#[cfg(feature = "std")]
impl<I, O> SlotPending<I, O> {
    /// 安装一个合规居留项 `T`（`T: PortCell<In=I, Out=O>` ⟹ `Conforms<Slot<I,O>>`）。
    pub fn install<T>(state: T::State) -> Self
    where
        T: PortCell<In = I, Out = O> + Send + 'static,
        T::State: Send + 'static,
    {
        let step: Step<I, O> = |s, input| {
            let st = s
                .downcast_mut::<T::State>()
                .expect("inhabitant state type matches");
            T::step(st, input)
        };
        SlotPending {
            state: Box::new(state),
            step,
        }
    }

    /// 授权（Ready → Live）：此后才可驱动。返回 [`SlotDrive`]（代 = 0）。
    pub fn commit(self) -> SlotDrive<I, O> {
        SlotDrive {
            state: self.state,
            step: self.step,
            generation: 0,
        }
    }
}

#[cfg(feature = "std")]
impl<I, O> SlotDrive<I, O> {
    /// 驱动一次（Finished）：`input` 流经已授权居留项，返回 `O`。
    pub fn drive(&mut self, input: I) -> O {
        (self.step)(&mut self.state, input)
    }

    /// 换装一个不同的合规居留项（运行期代换，存在化；**代递增** → 既有 `Seat` 过期）。
    ///
    /// **C5 语义边界**：本形态 = 已知无在途的换装——调用方**断言**旧居留项无未完成
    /// 工作（违反 = 声明者责任，A5）。旧状态可能持有在途的居留项（实现了
    /// [`Drainable`] 者）必须使用 [`Self::swap_and_drain`] 强制处置。
    pub fn swap<T>(&mut self, state: T::State)
    where
        T: PortCell<In = I, Out = O> + Send + 'static,
        T::State: Send + 'static,
    {
        self.generation = self.generation.wrapping_add(1);
        self.state = Box::new(state);
        self.step = |s, input| {
            let st = s
                .downcast_mut::<T::State>()
                .expect("inhabitant state type matches");
            T::step(st, input)
        };
    }

    /// 换装并**强制处置**旧状态（C5：在途值归属裁定）。
    ///
    /// 旧居留项状态（连同其未完成工作）`Box<dyn Any + Send>` 交给 `drain` 回调，
    /// 由调用方显式裁定归属——排空回传、随 `Closed` 回传（[`crate::checks::delivery::Delivery::Closed`]）、
    /// 或重建；**静默丢弃在类型上不可能**（回调必须存在）。回调的返回值即本方法的
    /// 返回值（调用方可回传处置裁决）。代递增（既有 `Seat` 过期）。
    pub fn swap_and_drain<T, R>(
        &mut self,
        state: T::State,
        drain: impl FnOnce(Box<dyn Any + Send>) -> R,
    ) -> R
    where
        T: PortCell<In = I, Out = O> + Send + 'static,
        T::State: Send + 'static,
    {
        let old = core::mem::replace(&mut self.state, Box::new(state));
        self.generation = self.generation.wrapping_add(1);
        self.step = |s, input| {
            let st = s
                .downcast_mut::<T::State>()
                .expect("inhabitant state type matches");
            T::step(st, input)
        };
        drain(old)
    }

    /// 当前代（诊断与测试可见；`Seat` 的校验基准）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 借用视图（[`Seat`]）：携带创建时的代；`swap` 后既有 `Seat` 驱动被拒绝。
    pub fn seat(&mut self) -> Seat<'_, I, O> {
        let generation = self.generation;
        Seat {
            drive: self,
            generation,
        }
    }

    /// 退役（Cleaned）：显式终结许可（drop 即拆除）。
    pub fn retire(self) {}
}

/// 借用的驱动视图（代戳）：`drive` 校验所驻 `SlotDrive` 的代未变。
#[cfg(feature = "std")]
pub struct Seat<'a, I, O> {
    drive: &'a mut SlotDrive<I, O>,
    generation: u64,
}

#[cfg(feature = "std")]
impl<'a, I, O> Seat<'a, I, O> {
    /// 驱动一次；若代不匹配（创建后发生过 `swap`），拒绝（陈旧引用契约）。
    pub fn drive(&mut self, input: I) -> O {
        assert_eq!(
            self.generation,
            self.drive.generation,
            "stale seat: the live slot was swapped while this seat existed"
        );
        (self.drive.step)(&mut self.drive.state, input)
    }

    /// 本席位持有的代。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 是否已陈旧：所驻 `SlotDrive` 的代与本席位创建时的代不一致（其间发生过 `swap`）。
    pub fn is_stale(&self) -> bool {
        self.generation != self.drive.generation
    }
}

/// 可排空居留项（C5：在途值归属的显式协议）。实现者声明"本居留项可能持有未完成的
/// 在途工作"；[`drain_pending`](Drainable::drain_pending) 在换装时把工作余量以值交出。
///
/// 不实现本 trait 的居留项由调用方在 [`SlotDrive::swap_and_drain`] 中显式处置——
/// 其状态仍可能持有在途（声明侧责任，A5），但**静默丢弃在类型上不可能**。
pub trait Drainable {
    /// 在途工作余量的类型。
    type Drain;
    /// 产出未完成的工作余量；`None` = 当前无在途。
    fn drain_pending(&mut self) -> Option<Self::Drain>;
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    struct Inc;
    impl PortCell for Inc {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x + 1
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
    fn commit_authorizes_drive() {
        // 许可生命周期：未 commit 的 Pending 无 drive 方法（模态①，编译期拒绝）——
        // 此测试验证授权后驱动正常。
        let pending = SlotPending::<i32, i32>::install::<Inc>(());
        let mut live = pending.commit();
        assert_eq!(live.drive(5), 6);
    }

    #[test]
    fn swap_bumps_generation_and_new_live_drives() {
        let mut live = SlotPending::<i32, i32>::install::<Inc>(()).commit();
        let g1 = live.generation();
        live.swap::<Double>(());
        let g2 = live.generation();
        assert_eq!(g2, g1 + 1, "swap 必须递增代");
        assert_eq!(live.drive(5), 10, "换装后驱动新居留项");
    }

    #[test]
    fn seat_holds_generation_contract() {
        let mut live = SlotPending::<i32, i32>::install::<Inc>(()).commit();
        let g = live.generation();
        let mut seat = live.seat();
        assert_eq!(seat.generation(), g);
        assert!(!seat.is_stale());
        assert_eq!(seat.drive(3), 4);
    }

    #[test]
    fn retire_drops_cleanly() {
        // Cleaned 阶段：retire 显式终结（drop 语义；编译期保证不可再驱动）。
        let live = SlotPending::<i32, i32>::install::<Inc>(()).commit();
        live.retire();
    }

    // ── C5：在途值归属（热替换语义）──────────────────────────────────────

    /// 带在途缓冲的测试居留项：In 追加进 State 缓冲，Out = 缓冲快照。
    struct Buffered;
    impl PortCell for Buffered {
        type In = i32;
        type Out = Vec<i32>;
        type State = Vec<i32>;
        #[inline(always)]
        fn step(buf: &mut Vec<i32>, x: i32) -> Vec<i32> {
            buf.push(x);
            buf.clone()
        }
    }
    // 状态类型声明 Drainable（换装交付的是状态本体；在途值以值交出）。
    impl Drainable for Vec<i32> {
        type Drain = Vec<i32>;
        fn drain_pending(&mut self) -> Option<Vec<i32>> {
            if self.is_empty() {
                None
            } else {
                Some(core::mem::take(self))
            }
        }
    }

    /// 换装目标居留项：In = i32、Out = Vec<i32>（同 Slot 类型），不积累。
    struct Sink;
    impl PortCell for Sink {
        type In = i32;
        type Out = Vec<i32>;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> Vec<i32> {
            vec![x]
        }
    }

    #[test]
    fn swap_and_drain_reclaims_inflight_to_closed() {
        // C5：在途值归属裁定——换装时旧居留项缓冲中的在途值经 drain 交出
        // （随 Closed 回传形态），不静默丢弃；换代；新居留项接续驱动。
        let mut live = SlotPending::<i32, Vec<i32>>::install::<Buffered>(vec![]).commit();
        let _ = live.drive(1);
        let _ = live.drive(2); // 缓冲在途 [1, 2]
        let g1 = live.generation();
        let verdict = live.swap_and_drain::<Sink, Option<Vec<i32>>>((), |mut old| {
            let st = old.downcast_mut::<Vec<i32>>().expect("buffered state");
            st.drain_pending()
        });
        assert_eq!(verdict, Some(vec![1, 2]), "在途值随 Closed 形态回传");
        assert_eq!(live.generation(), g1 + 1, "换装必须递增代");
        assert_eq!(live.drive(7), vec![7], "换装后驱动新居留项");
    }

    #[test]
    fn swap_and_drain_reports_clean_when_no_inflight() {
        // C5：无在途 = Clean（回调可得 None），换代照常。
        let mut live = SlotPending::<i32, Vec<i32>>::install::<Buffered>(vec![]).commit();
        let g1 = live.generation();
        let verdict = live.swap_and_drain::<Sink, Option<Vec<i32>>>((), |mut old| {
            let st = old.downcast_mut::<Vec<i32>>().expect("buffered state");
            st.drain_pending()
        });
        assert_eq!(verdict, None, "无在途 = 干净换装");
        assert_eq!(live.generation(), g1 + 1);
    }

    #[test]
    fn swap_and_drain_delegates_disposition_to_caller() {
        // C5：处置完全由调用方裁定——回调重建形态：拿走在途、以值交出（此处 = 计数）。
        let mut live = SlotPending::<i32, Vec<i32>>::install::<Buffered>(vec![]).commit();
        let _ = live.drive(9); // 在途 [9]
        let rebalanced = live.swap_and_drain::<Buffered, Vec<i32>>(vec![], |mut old| {
            let st = old.downcast_mut::<Vec<i32>>().expect("buffered state");
            let inflight = st.drain_pending().unwrap_or_default();
            vec![inflight.len() as i32] // 调用方裁定：在途计数以值移交，而非静默丢弃
        });
        assert_eq!(rebalanced, vec![1], "在途计数由调用方显式处置");
        assert_eq!(live.drive(5), vec![5], "新居留项（空缓冲）接续驱动");
    }
}