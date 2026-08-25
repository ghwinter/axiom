//! layered —— 多角色分层示例（B2）：一个 crate 内用三个文件演示"库作者 /
//! 拓扑集成方 / 部署方"三层组织；真实多 crate 工作区即此结构的模块化拆分
//! （`domain-cells` / `topology` / `deploy` 各一 crate，契约 = `PortCell` 类型
//! 与 `cell_core` 组合子）。
//!
//! - `cells.rs`：**域层**——纯 `PortCell` 定义（库作者暴露的角色；零物理依赖）；
//! - `topology.rs`：**拓扑层**——蓝图 = 组合子的类型别名（把单元合成形状）；
//! - `main.rs`：**部署层**——选载体、装配校验、驱动、观测。
//!
//! 分层法则：下层不引用上层；层间只经类型（`PortCell` 签名与组合子）通信。

mod cells;
mod topology;

use axiom_runtime::prelude_all::{CarrierCost, InlineCarrier, QueueCarrier};

fn main() {
    use axiom::cell_core::{Chain, PortCell};

    // 部署单元 = 整条拓扑 + 末端汇点（部署层可另组出口形态）。
    type Unit = Chain<topology::Pipeline, cells::AsIs>;

    // 部署层：选载体 + 装配校验（模态③成本门）+ 驱动。
    let link_inline = axiom_runtime::prelude_all::assemble_link::<Unit, cells::AsIs, InlineCarrier>(
        CarrierCost::ZeroAllocInline,
    )
    .expect("Inline 满足零分配预算");
    let (mut ua, mut ub) = (<Unit as PortCell>::State::default(), ());
    let out = link_inline(&mut ua, &mut ub, 3);
    println!("layered: inline drive -> {out}");

    // 同拓扑换物理（T6）：Queue 载体（每消息分配）预算下语义一致。
    let link_queue = axiom_runtime::prelude_all::assemble_link::<Unit, cells::AsIs, QueueCarrier>(
        CarrierCost::PerMessageAlloc,
    )
    .expect("Queue 满足每消息预算");
    let (mut qa, mut qb) = (<Unit as PortCell>::State::default(), ());
    assert_eq!(
        link_inline(&mut qa, &mut qb, 3),
        link_queue(&mut qa, &mut qb, 3),
        "T6：同图多物理语义一致"
    );

    println!("layered ok: 域/拓扑/部署三层分离，库作者暴露 PortCell、集成方写拓扑、部署方选物理");
}