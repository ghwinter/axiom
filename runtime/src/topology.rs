//! 物化后的活跃拓扑——runtime 持有的运行时状态。
//!
//! `materialize` 一次性计算两个派生索引，避免 tick 热路径重复建表：
//! - `machine_index`：机器名 → 索引（供 `mark_stopped` O(log M) 查表）；
//! - `in_degree`：每台机器的入边数（供停机传播递减，tick 时克隆一份）。

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;

use crate::erasure::RunningMachine;

/// 物化后的活跃拓扑——runtime 持有的运行时状态。
pub struct LiveTopology {
    pub machines: BTreeMap<String, Box<dyn RunningMachine>>,
    pub links: Vec<PhysicalLink>,
    pub topo_order: Vec<String>,
    /// 机器名 → `topo_order` 索引。tick 热路径用它把机器名映射到
    /// `in_degree` 的下标，避免每次 tick 重建映射表。
    pub machine_index: BTreeMap<String, usize>,
    /// 每台机器（按 `topo_order` 顺序）的入边数。tick 时克隆一份，
    /// `mark_stopped` 递减它——克隆是单次分配（与链路数无关），
    /// 保证 R002 的"每链接常数分配"不变量。
    pub in_degree: Vec<usize>,
}

pub struct PhysicalLink {
    pub src_machine: String,
    pub src_port: String,
    pub dst_machine: String,
    pub dst_port: String,
    /// 链接的物理语义（决定 Parallel 模式下的 channel 载体）。
    pub kind: axiom::link::LinkKind,
}
