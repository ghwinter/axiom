//! 四边接缝演示：物理边（进程级单窗）与洞清单的实例层消费侧。
//!
//! 身份：参考形态（implementer witness）——演示"第四边"接缝契约可被实例层
//! 接入（T6：同抽象契约、物理实现可替换），并遍历洞清单治理物（R004/E145 的
//! 消费侧）。物理边是唯一不是信息的边：allocator 进不了 `In/Out/State`（是物质
//! 不是信息），故只有"声明 + 观测"两面。
//!
//! 门控：`physical` feature（拉起语义层 `axiom-semantics/physical`，纯 core，
//! 不引 std 依赖）。本模块无 unsafe。

use axiom_semantics::checks::friction::{Friction, catalog};
use axiom_semantics::seams::physical::{PhysicalSnapshot, PhysicalWindow, assert_physical};

/// 参考形态：allocator 单窗（memory 实验的声明侧；axiom-exp 实验 7 快照曲线）。
pub struct InstanceAllocWindow;

/// 参考快照：live/peak 读数（纯只读；`Display` + `window` 身份）。
pub struct InstanceAllocSnapshot {
    /// 当前存活分配。
    pub live: u64,
    /// 观测窗内峰值。
    pub peak: u64,
}

impl core::fmt::Display for InstanceAllocSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "live={}B peak={}B", self.live, self.peak)
    }
}

impl PhysicalSnapshot for InstanceAllocSnapshot {
    fn window(&self) -> &'static str {
        <InstanceAllocWindow as PhysicalWindow>::WINDOW
    }
}

impl PhysicalWindow for InstanceAllocWindow {
    const WINDOW: &'static str = "alloc";
    const PROCESS_SINGLETON: bool = true; // 语言层强制唯一（#[global_allocator]）
    type Snapshot = InstanceAllocSnapshot;
}

/// 演示：物理边契约可满足（编译期判定）＋洞清单可索引（"物理单窗"洞的收容所）。
///
/// 返回 [`Friction::PhysicalSingleton`] 的收容所描述（第四边的洞 → 物理边接缝）。
pub fn demo() -> &'static str {
    assert_physical::<InstanceAllocWindow>();
    Friction::PhysicalSingleton.containment()
}

/// 演示：洞清单是可遍历的开放目录（`catalog()` 恰好六洞，`#[non_exhaustive]`）。
pub fn catalog_len() -> usize {
    catalog().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_window_declares_and_snapshots() {
        // 编译期判定：契约可满足；快照纯只读读数。
        assert_physical::<InstanceAllocWindow>();
        let snap = InstanceAllocSnapshot { live: 1024, peak: 4096 };
        assert_eq!(snap.window(), "alloc");
        assert!(format!("{snap}").contains("1024"), "Display 只读读数");
    }

    #[test]
    fn physical_singleton_hole_contained_by_physical_seam() {
        // 洞清单治理物：第四边的洞由物理边接缝收容（可索引、非空洞）。
        let containment = demo();
        assert!(
            containment.contains("seams::physical"),
            "物理单窗洞 → 物理边接缝收容所，got: {containment}"
        );
        assert_eq!(catalog().len(), 6, "六洞开放目录（non_exhaustive）");
        assert!(Friction::PhysicalSingleton.evidence().contains("memory"));
    }
}
