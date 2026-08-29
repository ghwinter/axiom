//! 拓扑层（B2）：蓝图 = 组合子的类型别名——把域层单元合成形状。
//! 只引用 `cell_core` 组合子与域层类型；无物理选择。

use axiom::cell_core::Chain;

use crate::cells::{Degrade, RateLimit, ScaleOk};

/// 全局类型别名（T1 布线合法性由组合子类型系统保持）。
/// `RateLimit → ScaleOk（Result 域适配）→ Degrade（降级政策）`。
pub type Pipeline = Chain<RateLimit, Chain<ScaleOk, Degrade>>;