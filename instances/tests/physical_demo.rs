#![cfg(feature = "physical")]
//! 四边接缝演示集成测试：实例层经物理边接缝接入第四边 + 洞清单可遍历
//! （R004/E145 消费侧；门控：`physical` feature，模式同 t6_crosscheck）。

use axiom_instances::backend::physical_demo::{
    InstanceAllocSnapshot, InstanceAllocWindow, catalog_len, demo,
};
use axiom_semantics::checks::friction::{Friction, catalog};
use axiom_semantics::seams::physical::{PhysicalSnapshot, PhysicalWindow};

#[test]
fn instance_window_is_a_process_singleton_alloc_window() {
    // 声明面：allocator 单窗（进程级唯一）+ 快照只读身份。
    assert!(<InstanceAllocWindow as PhysicalWindow>::PROCESS_SINGLETON);
    assert_eq!(<InstanceAllocWindow as PhysicalWindow>::WINDOW, "alloc");
    let snap = InstanceAllocSnapshot { live: 0, peak: 0 };
    assert_eq!(snap.window(), "alloc");
    assert_eq!(format!("{snap}"), "live=0B peak=0B", "Display 只读读数");
}

#[test]
fn gap_ledger_is_an_open_indexed_catalog() {
    // 洞清单治理物：六洞互异、可索引；物理单窗洞由物理边接缝收容。
    let all = catalog();
    assert_eq!(catalog_len(), 6);
    assert!(all.contains(&Friction::PhysicalSingleton));
    let mut seen = std::collections::BTreeSet::new();
    for f in all {
        assert!(seen.insert(*f), "目录互异：{f:?} 重复");
        assert!(!f.containment().is_empty(), "每洞有收容所");
        assert!(!f.evidence().is_empty(), "每洞有证据");
    }
    assert!(demo().contains("seams::physical"), "物理单窗洞 → 物理边收容所");
}
