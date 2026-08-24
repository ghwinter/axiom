//! 义务类类型系统（D1；meta-foundations 定义 1.6 与 runtime-constitution 蓝图）。
//!
//! 义务类 = 投递态 × 资源类 × 引用有效 × 生命周期 的参数化族。每条接缝**声明**其义务类，
//! 装配点按模态③ 校验（`flow::assemble_link` / `assemble_seam`）。
//!
//! 极小基律（A4）：资源类直接复用 [`CarrierCost`](crate::carrier::CarrierCost)（同构者不
//! 重复定义）；本模块只定义既有代码未覆盖的三个轴与模态标记。

use crate::carrier::CarrierCost;

/// 认识论强度模态（D2；meta 定义 1.3）。格 {①②③④} ∪ {∅（违例）}——每条义务恰占一格。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modality {
    /// ① 结构见证：类型级（如 `Conforms<Wire<A,B>>` 配对）。
    StructuralWitness,
    /// ② 常量见证：编译期（如 `assert_capacity_nonzero`）。
    ConstantWitness,
    /// ③ 部署验证：装配期（如 `assemble_link` 的成本校验）。
    DeploymentValidation,
    /// ④ 声明：不可判定/需机制，展出为假定（如 Moore 标记）。
    Declaration,
}

/// 投递态轴的机械化程度（delivery.rs 的模态声明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryKind {
    /// Full/Closed 已机械化（②③），Timeout/Cancelled 声明（④）。
    MechanizedFullClosed,
}

/// 引用有效轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// 无引用有效性义务。
    None,
    /// 代戳校验（slot.rs 阶段：`SlotLive::Seat`）。
    GenerationStamped,
}

/// 生命周期轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleKind {
    /// 无许可阶段。
    Unlicensed,
    /// typestate 许可：Pending → Live → retired（模态①）。
    Licensed,
}

/// 义务类：一条接缝的完整义务声明。
///
/// 默认值取保守形（fail-closed，Saltzer-Schroeder 1975；`CarrierCost` 默认 `External`
/// 同构）：未声明 = 无承诺。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObligationClass {
    /// 投递态轴（发送/接收四态机械化范围）。
    pub delivery: DeliveryKind,
    /// 资源类（复用 `CarrierCost` 序：ZeroAllocInline < PerMessageAlloc < External）。
    pub resource: CarrierCost,
    /// 引用有效轴（代戳）。
    pub reference: ReferenceKind,
    /// 生命周期轴（许可阶段）。
    pub lifecycle: LifecycleKind,
}

impl Default for ObligationClass {
    fn default() -> Self {
        ObligationClass {
            delivery: DeliveryKind::MechanizedFullClosed,
            resource: CarrierCost::External, // 保守默认：未声明不视为零分配
            reference: ReferenceKind::None,
            lifecycle: LifecycleKind::Unlicensed,
        }
    }
}

impl ObligationClass {
    /// 该义务类是否满足给定成本预算（declared ≤ budget；模态③ 判定主体）。
    pub fn satisfies_budget(&self, budget: CarrierCost) -> bool {
        self.resource <= budget
    }
}

// ═══════════════════════════════════════════════════════════════════
// 义务账本（A4/A5/A6 的机械：极小基律、诚实规则、外审件）
// ═══════════════════════════════════════════════════════════════════

/// 账目行：一条已落位义务的声明（类、模态、见证、符合性测试）。
///
/// 账本是对宪法（meta-foundations）的机器可读摘录；每行须有真实见证方（`witness`）与
/// 真实测试（`conformance`），否则按 A5 视为伪验证缺陷。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 接缝名（代码定位）。
    pub seam: &'static str,
    /// 义务的内容（一句话）。
    pub obligation: &'static str,
    /// 落位模态（A3）。
    pub modality: Modality,
    /// 见证实现（代码定位）。
    pub witness: &'static str,
    /// 符合性测试（代码定位）。
    pub conformance: &'static str,
}

/// 现行账本：全部已落位义务的枚举。新增义务须在此登记（A4 极小基律的审计面）。
pub const LEDGER: &[LedgerEntry] = &[
    LedgerEntry {
        seam: "flow::assemble_link",
        obligation: "载体成本预算于装配点校验",
        modality: Modality::DeploymentValidation,
        witness: "contract::validate_cost",
        conformance: "tests/deployment.rs: assemble_link_rejects_budget_violation",
    },
    LedgerEntry {
        seam: "flow::assemble_seam",
        obligation: "有界接缝成本+容量合并校验",
        modality: Modality::DeploymentValidation,
        witness: "contract::validate_seam",
        conformance: "tests/deployment.rs: assemble_seam_rejects_zero_capacity_at_deploy_time",
    },
    LedgerEntry {
        seam: "carrier::BoundedCarrier",
        obligation: "容量 CAP ≥ 1（拒绝 rendezvous 死锁形态）",
        modality: Modality::ConstantWitness,
        witness: "contract::assert_capacity_nonzero",
        conformance: "contract.rs 单元测试（std）",
    },
    LedgerEntry {
        seam: "flow::drive_feedback_inline",
        obligation: "FEED 仅依赖 State（内联无缓冲环的良定义）",
        modality: Modality::Declaration,
        witness: "contract::Moore",
        conformance: "contract.rs: inline_loop_drive_requires_moore_declaration",
    },
    LedgerEntry {
        seam: "profile::assemble_profile",
        obligation: "载体成本 ≤ 剖面预算（kernel=零分配 / service=每消息 / tool=外部）",
        modality: Modality::DeploymentValidation,
        witness: "contract::validate_cost",
        conformance: "profile.rs: kernel_profile_rejects_per_message_carriers",
    },
    LedgerEntry {
        seam: "law::PairLaw",
        obligation: "配对律：N 次投递 ↔ N 个区分性判定；已收 ≤ 已投",
        modality: Modality::DeploymentValidation,
        witness: "law.rs 探针（debug_assertions 门控）",
        conformance: "law.rs: pairing_law_holds_for_verdicts",
    },
    LedgerEntry {
        seam: "mailbox::BoundedMailbox",
        obligation: "容量 CAP ≥ 1（模态②门）+ 每生产者保底席位（反饥饿资源义务）",
        modality: Modality::ConstantWitness,
        witness: "contract::assert_capacity_nonzero",
        conformance: "mailbox.rs: capacity_semantics_include_per_producer_slots",
    },
    LedgerEntry {
        seam: "buffer::BoundedQueue::push",
        obligation: "断连时值随错误回传（不静默丢值）",
        modality: Modality::DeploymentValidation,
        witness: "delivery::Delivery::Closed",
        conformance: "tests/buffer.rs",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_class_is_conservative() {
        // fail-closed 默认：未声明 = External 资源 + 无引用/生命周期承诺。
        let c = ObligationClass::default();
        assert_eq!(c.resource, CarrierCost::External);
        assert!(!c.satisfies_budget(CarrierCost::ZeroAllocInline));
        assert!(!c.satisfies_budget(CarrierCost::PerMessageAlloc));
        assert!(c.satisfies_budget(CarrierCost::External));
    }

    #[test]
    fn ledger_every_row_has_witness_and_conformance() {
        // 账本完整性（A5/A6）：每行见证与测试皆非空。
        assert!(!LEDGER.is_empty());
        for row in LEDGER {
            assert!(!row.seam.is_empty(), "账目须有代码定位");
            assert!(!row.witness.is_empty(), "账目须有见证实现：{row:?}");
            assert!(!row.conformance.is_empty(), "账目须有符合性测试：{row:?}");
        }
    }

    #[test]
    fn ledger_modalities_respect_placement_law() {
        // 落位律（A3）抽查：可判定的义务不得写成声明。
        for row in LEDGER {
            if row.seam == "buffer::BoundedQueue::push" && row.modality == Modality::Declaration
            {
                panic!("push 断连可判定，不得声明为 ④")
            }
        }
    }
}