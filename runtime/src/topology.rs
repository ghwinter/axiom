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

/// ID 化路由索引（P0：动态路径热路径免字符串匹配与 String clone）。
///
/// materialize 期（含融合后）一次性构建：机器按 `topo_order` 序编号
/// （= `machine_index` 值），端口按 schema 的 `inputs()`/`outputs()` 序
/// 编号（`&'static str`，编译期已知）。tick 热路径：
/// - 队列传 `(machine_id, port_id)`——无 String clone；
/// - 停机检查按 ID 索引 `Vec<bool>`——O(1)；
/// - 路由按 ID 查 `route_by_src` 线性扫描输出端口表（输出端口通常 1-2）。
pub struct TopologyIds {
    /// machine_id → `[(src_port(&'static str), dst_machine_id, dst_port_id)]`。
    /// 运行时用 `ProcessResult::Yield` 的端口名（`&'static str`）直接比较。
    pub route_by_src: Vec<Vec<(&'static str, usize, u16)>>,
    /// machine_id → [输出端口名]（构建路由表与匹配用）。
    pub out_port_names: Vec<Vec<&'static str>>,
    /// machine_id → [输入端口名]（外部注入 String→ID、inject 还原用）。
    pub in_port_names: Vec<Vec<&'static str>>,
}

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
    /// 路由索引：src_machine → (src_port → (dst_machine, dst_port))。
    ///
    /// 物化期（含融合后）一次性构建，tick 热路径 O(log L) 查找——
    /// P2：消除 `route_target` 对 `links` 的 O(L) 线性扫描 + 每消息
    /// String clone（src 是已知编译期拓扑，路由是物化期事实，不应在
    /// 运行时重复扫描）。
    pub route_map: BTreeMap<String, BTreeMap<String, (String, String)>>,
    /// ID 化路由索引（P0）：`drive_sequential` 热路径用，消除 String
    /// clone 与字符串匹配。
    pub ids: TopologyIds,
}

pub struct PhysicalLink {
    pub src_machine: String,
    pub src_port: String,
    pub dst_machine: String,
    pub dst_port: String,
    /// 链接的物理语义（决定 Parallel 模式下的 channel 载体）。
    pub kind: axiom::link::LinkKind,
}
