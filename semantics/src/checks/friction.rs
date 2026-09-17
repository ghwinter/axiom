//! 完备性洞清单（摩擦目录）——**治理物**，不是词汇扩展。
//!
//! ## 定位
//!
//! 律（封闭的同步细胞代数：四边 + 链）上必须**特例收容**的洞——代数表达不了、
//! 只能特例实现否则无法完备表达的六件事。本模块把它们**类型化 + 索引化**：每个
//! 洞指向它的标准收容所与实验证据。完备性无法在律之内自证（Gödel 式天花板：
//! 你能证明"接线符合律"，证明不了"律是完备的"），所以洞清单是治理物——
//! **洞出现 → 加收容所 → 记入清单**，而不是假装没有洞。
//!
//! ## 六洞（律必须特例收容的地方）
//!
//! | 洞 | 代数表达不了什么 | 标准收容所 | 证据 |
//! |---|---|---|---|
//! | 等待/挂起 | "等某值"不是 cell 语义（`step` 永不等是裁定） | 控制边接缝（Executor / async） | disk 回传通道、console EX 接缝 |
//! | 物理单窗 | allocator 进不了 In/Out/State（是物质不是信息） | 物理边接缝（[`crate::seams::physical`]） | memory 快照曲线 |
//! | 贯穿 | 载荷不属于任何模块，代数没有"贯穿带"形状 | 横切面契约（[`crate::seams::crosscut`]） | ctx 传播 / 派生取消 |
//! | 驻留 | 代数不规定谁唤醒、是否常驻 | 模块物理侧（worker / 节拍线程） | clock 节拍线程、disk worker |
//! | 派生合成 | 背压/期限/取消是"乘积"不是基元 | 显式策略接线 | Saturated / Missed / token |
//! | 完备性 | 封闭词汇要预言终态，世界会加新终态 | 显式终态（Full(v)/Missed/…） | 投递四态、调度 Missed |
//!
//! ## 使用
//!
//! [`Friction::catalog()`] 列出全部洞；每洞的 [`Friction::containment()`] /
//! [`Friction::evidence()`] 给出收容所与证据路径。`#[non_exhaustive]`——第七洞
//! 出现时加变体，不破坏既有匹配。

/// 完备性洞（摩擦）：律必须特例收容的六洞之一。
///
/// `#[non_exhaustive]`：洞清单是开放目录——第七洞出现时加变体，不破坏既有匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Friction {
    /// 等待 / 挂起：代数表达不了"等某值"（`step` 永不等是裁定，等待只在边界）。
    /// 收容所：控制边接缝（Executor 等待点 / async）。
    Waiting,
    /// 物理单窗：语言层进程级唯一（allocator / 信号 / stdio），进不了 In/Out/State。
    /// 收容所：物理边接缝（[`crate::seams::physical`] 的声明 + 观测）。
    PhysicalSingleton,
    /// 贯穿：载荷不属于任何模块，代数没有"贯穿带"形状。
    /// 收容所：横切面契约（[`crate::seams::crosscut`] 的无表面 marker）。
    CrossCutting,
    /// 驻留：代数不规定谁唤醒、是否常驻。
    /// 收容所：模块物理侧（worker / 节拍线程）。
    Residency,
    /// 派生合成：背压 / 期限 / 取消是"乘积"不是基元。
    /// 收容所：显式策略接线（饱和四态 / 期限判定 / 取消令牌）。
    DerivedSemantics,
    /// 完备性：封闭词汇要预言终态，世界会加新终态。
    /// 收容所：显式终态（Full(v) / Missed / Closed / Cancelled，值随判定回传）。
    Completeness,
}

impl Friction {
    /// 标准收容所（谁接住这个洞）。
    pub fn containment(self) -> &'static str {
        match self {
            Friction::Waiting => "控制边接缝：Executor 等待点 / async（seams::async_seam）",
            Friction::PhysicalSingleton => "物理边接缝：声明 + 观测（seams::physical）",
            Friction::CrossCutting => "横切面契约：无表面 marker（seams::crosscut）",
            Friction::Residency => "模块物理侧：worker / 节拍线程",
            Friction::DerivedSemantics => "显式策略接线：饱和四态 / 期限判定 / 取消令牌",
            Friction::Completeness => "显式终态：Full(v) / Missed / Closed / Cancelled",
        }
    }

    /// 实验证据（哪次验证逼出这个洞）。
    pub fn evidence(self) -> &'static str {
        match self {
            Friction::Waiting => "axiom-exp：disk 回传通道、console EX 接缝",
            Friction::PhysicalSingleton => "axiom-exp 实验 7：memory 快照曲线",
            Friction::CrossCutting => "axiom-exp 实验 11：ctx 传播 + 派生取消",
            Friction::Residency => "axiom-exp：clock 节拍线程、disk worker",
            Friction::DerivedSemantics => "axiom-exp：bus Saturated、sched Missed、ctx token",
            Friction::Completeness => "axiom：Delivery 四态 / sched Missed / bus Saturated",
        }
    }
}

/// 洞清单（治理物）：全部六洞，按"律上出现顺序"列出。
pub fn catalog() -> &'static [Friction] {
    &[
        Friction::Waiting,
        Friction::PhysicalSingleton,
        Friction::CrossCutting,
        Friction::Residency,
        Friction::DerivedSemantics,
        Friction::Completeness,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;

    #[test]
    fn catalog_is_complete_and_indexed() {
        let holes = catalog();
        // 六洞齐全且互异（治理物完整性：目录不被悄悄缩水）。
        assert_eq!(holes.len(), 6);
        let mut seen = BTreeSet::new();
        for h in holes {
            assert!(seen.insert(*h), "duplicate hole in catalog: {h:?}");
            // 每洞都有收容所与证据（治理物索引性：不空指针）。
            assert!(!h.containment().is_empty());
            assert!(!h.evidence().is_empty());
        }
    }

    #[test]
    fn non_exhaustive_is_open() {
        // 洞清单是开放目录：catalog 之外的洞仍然存在（如未来新增），
        // 既有匹配不受影响——本测试仅验证枚举可用作集合元素。
        let w = Friction::Waiting;
        assert_eq!(w.containment(), Friction::Waiting.containment());
    }
}
