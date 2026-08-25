//! 义务类类型系统（D1；meta-foundations 定义 1.6 与 runtime-constitution 蓝图）。
//!
//! 义务类 = 投递态 × 资源类 × 引用有效 × 生命周期 的参数化族。每条接缝**声明**其义务类，
//! 装配点按模态③ 校验（`flow::assemble_link` / `assemble_seam`）。
//!
//! 极小基律（A4）：资源类直接复用 [`CarrierCost`](crate::carrier::CarrierCost)（同构者不
//! 重复定义）；本模块只定义既有代码未覆盖的三个轴与模态标记。

use crate::carrier::CarrierCost;
#[cfg(feature = "std")]
use crate::delivery::Delivery;
use crate::flow;
#[cfg(feature = "std")]
use crate::law::PairLaw;

// ═══════════════════════════════════════════════════════════════════
// 见证探针的最小居留项（仅探针使用；不进入任何公共 API）
// ═══════════════════════════════════════════════════════════════════

/// 探针用单元（Inc）。
pub struct ProbeInc;
impl axiom::cell_core::PortCell for ProbeInc {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(1)
    }
}

/// 探针用单元（×3）。
pub struct ProbeTriple;
impl axiom::cell_core::PortCell for ProbeTriple {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_mul(3)
    }
}

/// 探针用失败单元（`Result` 语域）。
pub struct ProbeFail;
impl axiom::cell_core::PortCell for ProbeFail {
    type In = i32;
    type Out = Result<i32, &'static str>;
    type State = ();
    fn step(_: &mut (), _: i32) -> Result<i32, &'static str> {
        Err("probe")
    }
}

/// 探针用汇单元。
pub struct ProbeSink;
impl axiom::cell_core::PortCell for ProbeSink {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x
    }
}

/// 探针用 Moore 体。
pub struct ProbeMooreBody;
impl axiom::cell_core::PortCell for ProbeMooreBody {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(1)
    }
}
impl crate::contract::Moore for ProbeMooreBody {}

/// 探针用 Moore 回喂。
pub struct ProbeMooreFeed;
impl axiom::cell_core::PortCell for ProbeMooreFeed {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(1)
    }
}
impl crate::contract::Moore for ProbeMooreFeed {}

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

/// 账目行：一条已落位义务的声明（类、模态、见证、符合性测试）＋**可执行见证探针**。
///
/// 账本是对宪法（meta-foundations）的机器可读摘录；每行须有真实见证方（`witness`）与
/// 真实测试（`conformance`），否则按 A5 视为伪验证缺陷。
///
/// **可执行性（C11）**：`probe` 在运行期执行该行的**见证符号本身**（非接缝）——若见证
/// 被改名/删除，LEDGER 字面量编译失败（模态①）；探针返回 false 则测试失败（③）。
/// 这使"账实分离"在两个时刻都被机械拦截。
#[derive(Debug, Clone, Copy)]
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
    /// 见证探针：执行一次见证符号的最小调用，恒返 true（存在性＋活性双检）。
    pub probe: fn() -> bool,
}

/// 现行账本：全部已落位义务的枚举。新增义务须在此登记（A4 极小基律的审计面）。
pub const LEDGER: &[LedgerEntry] = &[
    LedgerEntry {
        seam: "flow::assemble_link",
        obligation: "载体成本预算于装配点校验",
        modality: Modality::DeploymentValidation,
        witness: "contract::validate_cost",
        conformance: "tests/deployment.rs: assemble_link_rejects_budget_violation",
        probe: || crate::contract::validate_cost::<ProbeInc, ProbeTriple, crate::carrier::InlineCarrier>(CarrierCost::External).is_ok(),
    },
    LedgerEntry {
        seam: "flow::assemble_seam",
        obligation: "有界接缝成本+容量合并校验",
        modality: Modality::DeploymentValidation,
        witness: "contract::validate_seam",
        conformance: "tests/deployment.rs: assemble_seam_rejects_zero_capacity_at_deploy_time",
        probe: || crate::contract::validate_seam::<ProbeInc, ProbeTriple, crate::carrier::InlineCarrier, 4>(CarrierCost::External).is_ok(),
    },
    LedgerEntry {
        seam: "carrier::BoundedCarrier",
        obligation: "容量 CAP ≥ 1（拒绝 rendezvous 死锁形态）",
        modality: Modality::ConstantWitness,
        witness: "contract::assert_capacity_nonzero",
        conformance: "contract.rs 单元测试（std）",
        probe: || { crate::contract::assert_capacity_nonzero::<4>(); true },
    },
    LedgerEntry {
        seam: "flow::drive_feedback_inline",
        obligation: "FEED 仅依赖 State（内联无缓冲环的良定义）",
        modality: Modality::Declaration,
        witness: "contract::Moore",
        conformance: "contract.rs: inline_loop_drive_requires_moore_declaration",
        probe: || flow::drive_feedback_inline::<ProbeMooreBody, ProbeMooreFeed>(&mut (), &mut (), 5) == 8,
    },
    LedgerEntry {
        seam: "carrier::ResultCarrier/MaybeCarrier",
        obligation: "Ok 直通 B、Err 短路（B 不执行；失败为值、step 保持全函数）",
        modality: Modality::DeploymentValidation,
        witness: "carrier::drive_try_carrier",
        conformance: "carrier.rs: short_circuit_ok_passes_and_err_skips",
        probe: || crate::carrier::drive_try_carrier::<crate::carrier::ResultCarrier, ProbeFail, ProbeSink, i32, &'static str>(&mut (), &mut (), 1).is_err(),
    },
    LedgerEntry {
        seam: "profile::assemble_profile",
        obligation: "载体成本 ≤ 剖面预算（kernel=零分配 / service=每消息 / tool=外部）",
        modality: Modality::DeploymentValidation,
        witness: "contract::validate_cost",
        conformance: "profile.rs: kernel_profile_rejects_per_message_carriers",
        probe: || crate::profile::assemble_profile::<crate::profile::ToolProfile, ProbeInc, ProbeTriple, crate::carrier::InlineCarrier>().is_ok(),
    },

    LedgerEntry {
        seam: "mailbox::BoundedMailbox",
        obligation: "容量 CAP ≥ 1（模态②门）+ 每生产者保底席位（反饥饿资源义务）",
        modality: Modality::ConstantWitness,
        witness: "contract::assert_capacity_nonzero",
        conformance: "mailbox.rs: capacity_semantics_include_per_producer_slots",
        probe: || { crate::contract::assert_capacity_nonzero::<8>(); true },
    },
    LedgerEntry {
        seam: "ring::BoundedRing",
        obligation: "容量 CAP ≥ 1（模态②门）＋ 计数配对律（N 次成功 push ⟹ 恰 N 次可得 pop）",
        modality: Modality::ConstantWitness,
        witness: "contract::assert_capacity_nonzero",
        conformance: "ring.rs: counter_wraparound_cap3 / long_run_counter_law_via_pairlaw_shape",
        probe: || {
            let mut r = crate::ring::BoundedRing::<i32, 2>::new();
            let mut pushes = 0u64;
            let mut pops = 0u64;
            for i in 0..8i32 {
                if r.push(i).is_ok() { pushes += 1; }
                if r.pop().is_ok() { pops += 1; }
            }
            assert_eq!(pops, pushes);
            true
        },
    },
];


/// std 特性下的追加账目（依赖 `delivery`/`law` 模块；no_std 构建不含此两行）。
#[cfg(feature = "std")]
pub const LEDGER_STD_EXTRA: &[LedgerEntry] = &[
    LedgerEntry {
        seam: "law::PairLaw",
        obligation: "配对律：N 次投递 ↔ N 个区分性判定；已收 ≤ 已投",
        modality: Modality::DeploymentValidation,
        witness: "law.rs 探针（debug_assertions 门控）",
        conformance: "law.rs: pairing_law_holds_for_verdicts",
        probe: || { let l = PairLaw::new(); l.on_send(); l.on_verdict(&Delivery::Delivered::<i32>); l.assert_pairing(); true },
    },    LedgerEntry {
        seam: "event::pump_events",
        obligation: "配对律：N 条事件 ↔ N 个判定（delivered+dropped）；失败也是数据（不短路吞值）；消费端断连 ⟹ 停止拉取（拆除，不静默延续）；块容量 N≥1（模态②门，退化态拒绝）",
        modality: Modality::DeploymentValidation,
        witness: "event::EventPumpStats / event::split_lines",
        conformance: "event.rs: pump_pair_law_totals_match / pump_teardown_stops_pulling_and_counts_dropped",
        probe: || {
            use std::io::Cursor;
            let mut source = crate::event::ChunkSource::<Cursor<&[u8]>, _, String, i32, 16>::new(
                Cursor::new(&b"1\n2\n3\n"[..]),
                String::new(),
                |buf: &mut String, chunk: &[u8]| {
                    crate::event::split_lines(buf, chunk)
                        .into_iter()
                        .map(|l| l.trim().parse::<i32>().unwrap_or(0))
                        .collect()
                },
            );
            let mut a = ();
            let stats = crate::event::pump_events::<crate::obligation::ProbeFail, _, _>(
                &mut a,
                &mut source,
                |_outcome| crate::event::PushVerdict::Delivered,
            );
            stats.delivered == 3 && stats.total() == 3 && stats.dropped == 0
        },
    },
    LedgerEntry {
        seam: "buffer::BoundedQueue::push",
        obligation: "断连时值随错误回传（不静默丢值）",
        modality: Modality::DeploymentValidation,
        witness: "delivery::Delivery::Closed",
        conformance: "tests/buffer.rs",
        probe: || matches!(Delivery::Closed(1i32), Delivery::Closed(_)),
    },];


/// 全量账本访问：默认特性下含 std 追加行，no_std 下仅核心行。
pub fn ledger_rows() -> alloc::boxed::Box<dyn Iterator<Item = &'static LedgerEntry>> {
    #[cfg(feature = "std")]
    {
        alloc::boxed::Box::new(LEDGER.iter().chain(LEDGER_STD_EXTRA.iter()))
    }
    #[cfg(not(feature = "std"))]
    {
        alloc::boxed::Box::new(LEDGER.iter())
    }
}
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
        for row in ledger_rows() {
            assert!(!row.seam.is_empty(), "账目须有代码定位");
            assert!(!row.witness.is_empty(), "账目须有见证实现：{row:?}");
            assert!(!row.conformance.is_empty(), "账目须有符合性测试：{row:?}");
        }
    }

    #[test]
    fn ledger_probes_execute_true() {
        // 账实合一（C11）：每行探针执行其见证符号本身——见证被改名/删除 ⟹ 本文件编译
        // 失败（模态①）；探针返 false ⟹ 此测试失败（③）。
        for row in ledger_rows() {
            assert!((row.probe)(), "见证探针失败：{}", row.seam);
        }
    }

    #[test]
    fn ledger_modalities_respect_placement_law() {
        // 落位律（A3）抽查：可判定的义务不得写成声明。
        for row in ledger_rows() {
            if row.seam == "buffer::BoundedQueue::push" && row.modality == Modality::Declaration
            {
                panic!("push 断连可判定，不得声明为 ④")
            }
        }
    }
}
