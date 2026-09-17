//! 物理边接缝（四边之四）——进程级单窗的**声明 + 观测**契约。
//!
//! ## 概念归属（§8.3 封闭判据）
//!
//! 四边模型：数据边（[`axiom::cell_core::Wire`] 对偶配对）、控制边
//! （[`crate::seams::async_seam`] 的 `Executor`）、观测边（[`crate::seams::telemetry`]）、
//! **物理边**（本接缝）。物理边是**唯一不是信息的边**：allocator / 信号 / stdio /
//! 环境变量是**物质单窗**，语言层强制进程级唯一（`#[global_allocator]` 只能存在一个）。
//! 它进不了 `In/Out/State`——不是 cell，不立表面，只能被**声明 + 观测**。
//!
//! ## 边界诚实声明
//!
//! 物理边不是"限额管理器"：它只提供**声明 + 观测**。限额 / 压力感知若需要，须显式
//! 接线（快照查询面 → 模块决策），不在本接缝内隐含任何治理——与 axiom-exp 实验 7
//! （memory 模块）的结论一致：memory 是"观测 + 统一分配通道"，不是"限额管理器"。
//!
//! ## 契约
//!
//! - [`PhysicalWindow`]：模块对进程级单窗的**占用声明**（身份 + 是否语言层唯一）；
//! - [`PhysicalSnapshot`]：**纯只读读数**（数据边流出），不得包含可执行语义——快照
//!   是观测的产物，不是命令的入口。
//!
//! ## 模态
//!
//! 声明是模态①（类型级令牌，编译期可判定）；快照是模态③（运行期可见证读数）。
//! 两者皆无伪判定：本接缝不提供"分配上限""压力分级"等未兑现的 API（对齐
//! [`crate::checks::delivery`] 的 A5 诚实规则——不伪造见证）。
//!
//! ## 参考实现
//!
//! axiom-exp 实验 7 的 memory 模块：`#[global_allocator]` 挂 TrackingAlloc +
//! 快照曲线（baseline → start → write 1MiB → read 1MiB → stop 回落）。快照的
//! 取得走数据边（`Snapshot` cell 查询面），本接缝只声明"单窗 + 快照形状"。

/// 物理快照：进程级单窗的**纯只读读数**（数据边流出）。
///
/// 约束：只读 + 可展示；**不得携带可执行语义**——快照是观测的产物，不是命令入口
/// （限额 / 压力感知须另走显式接线，见模块文档）。
pub trait PhysicalSnapshot: core::fmt::Display {
    /// 单窗身份（与所属 [`PhysicalWindow::WINDOW`] 一致）。
    fn window(&self) -> &'static str;
}

/// 物理边接缝：模块对**进程级单窗**的占用声明 + 观测契约。
///
/// 实现者不实现 [`axiom::cell_core::PortCell`]——物理边没有 In/Out/State 表面；
/// 本 trait 是"我占用这个语言层单窗"的姿态声明 + 快照查询面的形状声明。
pub trait PhysicalWindow {
    /// 单窗身份（allocator / 信号 / stdio / 环境…）。进程内唯一。
    const WINDOW: &'static str;
    /// 语言层强制唯一（如 `#[global_allocator]` 只能存在一个）？
    const PROCESS_SINGLETON: bool;
    /// 观测快照（数据边流出；纯只读读数）。
    type Snapshot: PhysicalSnapshot;
}

/// 编译期判定：`W` 满足物理边接缝契约（进程级单窗声明 + 可观测）。
pub fn assert_physical<W: PhysicalWindow>() {}

#[cfg(test)]
mod tests {
    use alloc::format;
    use super::*;

    /// 参考形态：allocator 单窗（memory 实验的声明侧）。
    struct AllocWindow;

    /// 参考快照：live/peak/总分配/分配次数（纯只读读数）。
    struct AllocSnapshot {
        live: u64,
        peak: u64,
    }

    impl core::fmt::Display for AllocSnapshot {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "live={}B peak={}B", self.live, self.peak)
        }
    }

    impl PhysicalSnapshot for AllocSnapshot {
        fn window(&self) -> &'static str {
            <AllocWindow as PhysicalWindow>::WINDOW
        }
    }

    impl PhysicalWindow for AllocWindow {
        const WINDOW: &'static str = "alloc";
        const PROCESS_SINGLETON: bool = true;
        type Snapshot = AllocSnapshot;
    }

    #[test]
    fn window_declares_and_snapshots() {
        // 编译期判定：契约可满足。
        assert_physical::<AllocWindow>();
        // 快照是纯只读读数（Display + window 身份）。
        let snap = AllocSnapshot { live: 1024, peak: 4096 };
        assert_eq!(snap.window(), "alloc");
        assert!(format!("{snap}").contains("1024"));
    }
}
