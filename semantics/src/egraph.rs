//! **egraph — ≡_eg 商机器（组合律即重写规则）**
//!
//! 地位（本体论审计 F2 的代码落点）：结合律等组合律在 Rust 类型系统里是
//! 行为同构而非类型相等——**商空间 T(Σ)/≡_beh 没有被任何机器执行**，这是
//! 审计认定的"必经之路"。本模块给出该机器的实现：一个自足的 e-graph
//! （hashcons + 并查集 + **惰性同余重建**），零依赖、alloc 即可。
//!
//! - **输入**：两个 [`Term`](crate::term::Term)（经 [`crate::term::Reify`]
//!   从类型级蓝图提取）；
//! - **规则**：组合律的具体化——结合律、单位律（见 `RULES`）；
//! - **输出**：商类成员判定 [`EGraph::equivalent`]。
//!
//! ## 算法来源（听证 E 裁决：选项 (a)——自建而学 egg 算法）
//!
//! 同余闭包采用 egg（egraphs-good/egg，论文 arXiv:2004.03082）的**惰性
//! 重建**而非全图扫描：union 时只把被并类的父 e-node 压入 `pending`
//! 工作表；[`EGraph::rebuild`] 逐个把父 e-node 的子引用规范化后重新
//! hashcons，撞车即合并。近线性，且把两个关注点分离：
//!
//! - **同余闭包**是数据结构不变量（`rebuild` 总是可恢复、必终止）；
//! - **规则饱和**可能不终止（交给 [`EGraph::saturate`] 的预算管理）。
//!
//! 阅读记录与三映射见 `tmp/research/egg.md`（听证 E：短期选项 (a)，
//! 中期分析器械独立 crate 可依赖 egg）。
//!
//! ## 诚实边界（与审计/frontier 登记一致）
//!
//! 1. **预算有界饱和**：重写集的合流性与终止性**未证明**（审计开放项；
//!    egg 的工程答案同样是限额与调度而非证明）。未在预算内合流时判定
//!    可能给出**假阴性**——不产生假阳性：等价类只在规则明确重写或同余
//!    成立时合并。真值方向偏保守。
//! 2. **无类型标签**：[`Term`](crate::term::Term) 不携带端口类型，全部
//!    `Id`/`Swap` 共享同一节点。对当前规则集这是可靠的：结合律与单位律
//!    的重写在良形项上保真（良形性由 reify 前的 Rust 类型系统保证）。
//!    依赖类型上下文的律（对称自然性、迹方程、余单位方程）**刻意未收录**。
//!    typed-Term 开放项已按 egg 的 `Analysis` 模式改写：端口类型作为
//!    e-class 分析数据（`make` 推断 / `merge` 归并），`Term` 保持无类型；
//!    该钩子随类型依赖规则一起落地（登记项，不在无消费时不实现）。
//! 3. **模态③器械**：判定结果覆盖"被提交的项 + 已收录的规则"，不是
//!    对 ∀ 项的证明；它是行为等价测试（core `laws`）之上的代数化升级，
//!    不是其替代。

use crate::term::Term;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

// ── 1. e-node（带等价类引用的构造子）──────────────────────────────

/// e-node：构造子标签 + 子等价类 id。项在图中的规范形态。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ENode {
    Cell(&'static str),
    Id,
    Swap,
    Discard,
    Duplicate,
    Chain(u32, u32),
    Par(u32, u32),
    Broadcast(u32, u32, u32),
    Merge(u32, u32, u32),
    Diamond(u32, u32, u32, u32),
    Feedback(u32, u32),
    Choice(u32, u32),
    Opt(u32),
    Rep(usize, u32),
}

// ── 2. e-graph ────────────────────────────────────────────────────

/// e-graph：hashcons + 并查集 + 父表 + 惰性重建工作表。
/// 零依赖、确定性（BTreeMap 序）。
#[derive(Debug, Default)]
pub struct EGraph {
    /// 全部 e-node（子引用可能是过期类 id；rebuild 时重新规范化）。
    nodes: Vec<ENode>,
    /// 并查集（路径减半）。
    parent: Vec<u32>,
    /// hashcons memo：规范化 e-node → 成员 e-node id（非类根，egg 同款约定）。
    memo: BTreeMap<ENode, u32>,
    /// 类根 → 成员 e-node id 集合。
    members: BTreeMap<u32, BTreeSet<u32>>,
    /// 类根 → 引用该类的 e-node id 列表（union 时压入 pending 的原料）。
    parents: Vec<Vec<u32>>,
    /// 待重建队列：union 后需要重新 hashcons 的父 e-node id。
    pending: Vec<u32>,
    /// 读操作前置条件：不变量（同余 + e-node 唯一性）是否已恢复。
    clean: bool,
}

impl EGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// 并查集寻根（路径减半，迭代实现）。
    pub fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let g = self.parent[self.parent[x as usize] as usize];
            self.parent[x as usize] = g;
            x = g;
        }
        x
    }

    /// 合并两个等价类；返回是否发生了新合并。确定性：保留较小编号为根。
    ///
    /// 被并类的全部父 e-node 进入 pending——同余由此**惰性**传播，
    /// 而非立即扫描全图（egg 核心机制）。
    pub fn union(&mut self, a: u32, b: u32) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }
        let (keep, drop_) = if ra <= rb { (ra, rb) } else { (rb, ra) };
        self.parent[drop_ as usize] = keep;
        self.clean = false;
        let moved_members = self.members.remove(&drop_).unwrap_or_default();
        self.members.entry(keep).or_default().extend(moved_members);
        let moved_parents = core::mem::take(&mut self.parents[drop_ as usize]);
        self.pending.extend(moved_parents.iter().copied());
        self.parents[keep as usize].extend(moved_parents);
        true
    }

    /// 插入 e-node（子 id 须已是规范根）；hashcons 去重并登记父表。
    pub fn add_enode(&mut self, e: ENode) -> u32 {
        if let Some(&id) = self.memo.get(&e) {
            return id;
        }
        let id = self.nodes.len() as u32;
        // 登记父表：子类的父列表加入本节点。
        let mut child_roots = Vec::new();
        e.for_each_child(&mut |c| child_roots.push(c));
        for c in child_roots {
            self.parents[c as usize].push(id);
        }
        self.nodes.push(e.clone());
        self.parent.push(id);
        self.parents.push(Vec::new());
        self.memo.insert(e, id);
        self.members.entry(id).or_default().insert(id);
        self.clean = false;
        id
    }

    /// 加入一个项（递归加入子项），返回其等价类 id。
    pub fn add(&mut self, t: &Term) -> u32 {
        let e = match t {
            Term::Cell(n) => ENode::Cell(n),
            Term::Id => ENode::Id,
            Term::Swap => ENode::Swap,
            Term::Discard => ENode::Discard,
            Term::Duplicate => ENode::Duplicate,
            Term::Chain(a, b) => ENode::Chain(self.add(a), self.add(b)),
            Term::Par(a, b) => ENode::Par(self.add(a), self.add(b)),
            Term::Broadcast(a, b, c) => ENode::Broadcast(self.add(a), self.add(b), self.add(c)),
            Term::Merge(a, b, c) => ENode::Merge(self.add(a), self.add(b), self.add(c)),
            Term::Diamond(a, b, c, d) => {
                ENode::Diamond(self.add(a), self.add(b), self.add(c), self.add(d))
            }
            Term::Feedback(a, b) => ENode::Feedback(self.add(a), self.add(b)),
            Term::Choice(a, b) => ENode::Choice(self.add(a), self.add(b)),
            Term::Opt(a) => ENode::Opt(self.add(a)),
            Term::Rep(n, a) => ENode::Rep(*n, self.add(a)),
        };
        let id = self.add_enode(e);
        self.find(id)
    }

    /// 一个等价类的全部成员（规范化的 e-node 视图，快照）。
    fn class_nodes(&mut self, class: u32) -> Vec<(u32, ENode)> {
        let class = self.find(class);
        let member_ids: Vec<u32> = self
            .members
            .get(&class)
            .map(|m| m.iter().copied().collect())
            .unwrap_or_default();
        member_ids
            .into_iter()
            .map(|id| {
                let e = self.nodes[id as usize].clone();
                (id, self.canonicalize(e))
            })
            .collect()
    }

    /// 把 e-node 的子引用换成当前规范根（只读跟随；链压缩交给 find）。
    fn canonicalize(&self, e: ENode) -> ENode {
        fn root(parent: &[u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                x = parent[x as usize];
            }
            x
        }
        match e {
            ENode::Chain(a, b) => ENode::Chain(root(&self.parent, a), root(&self.parent, b)),
            ENode::Par(a, b) => ENode::Par(root(&self.parent, a), root(&self.parent, b)),
            ENode::Broadcast(a, b, c) => ENode::Broadcast(
                root(&self.parent, a),
                root(&self.parent, b),
                root(&self.parent, c),
            ),
            ENode::Merge(a, b, c) => ENode::Merge(
                root(&self.parent, a),
                root(&self.parent, b),
                root(&self.parent, c),
            ),
            ENode::Diamond(a, b, c, d) => ENode::Diamond(
                root(&self.parent, a),
                root(&self.parent, b),
                root(&self.parent, c),
                root(&self.parent, d),
            ),
            ENode::Feedback(a, b) => ENode::Feedback(root(&self.parent, a), root(&self.parent, b)),
            ENode::Choice(a, b) => ENode::Choice(root(&self.parent, a), root(&self.parent, b)),
            ENode::Opt(a) => ENode::Opt(root(&self.parent, a)),
            ENode::Rep(n, a) => ENode::Rep(n, root(&self.parent, a)),
            leaf => leaf,
        }
    }

    /// 恢复同余不变量（egg 的 `rebuild`）：逐个处理 pending 父 e-node，
    /// 子引用规范化后重新 hashcons——撞车即合并（合并又可能产生新的
    /// pending，循环直至队列清空）。返回同余引发的合并次数。
    ///
    /// 修改图（add/union）之后、任何查询之前必须调用；[`EGraph::clean`]
    /// 反映其状态。近线性（每个 e-node 每轮至多重哈希一次）。
    pub fn rebuild(&mut self) -> usize {
        let mut n_unions = 0;
        while let Some(nid) = self.pending.pop() {
            let node = self.canonicalize(self.nodes[nid as usize].clone());
            // Some(prev)：规范化形态已被见过的成员占位——撞车即同余合并；
            // None：首次见此规范化形态。
            if let Some(prev) = self.memo.insert(node, nid)
                && self.union(prev, nid)
            {
                n_unions += 1;
            }
        }
        self.clean = true;
        n_unions
    }

    /// 一轮饱和：对每个根类应用全部规则；随后 [`EGraph::rebuild`] 惰性
    /// 恢复同余。返回本轮是否产生了新的节点或合并。
    fn saturate_once(&mut self) -> bool {
        let mut added = false;
        let class_ids: Vec<u32> = (0..self.nodes.len() as u32).collect();
        for class in class_ids {
            if self.find(class) != class {
                continue; // 非根：类已并入他类
            }
            let snapshot = self.class_nodes(class);
            for (node_id, enode) in snapshot {
                for rule in RULES {
                    if rule(self, node_id, &enode) {
                        added = true;
                    }
                }
            }
        }
        let merged = self.rebuild();
        added || merged > 0
    }

    /// 饱和至不动点或预算耗尽。返回实际执行的轮数。
    pub fn saturate(&mut self, max_rounds: usize) -> usize {
        let mut rounds = 0;
        while rounds < max_rounds && self.saturate_once() {
            rounds += 1;
        }
        rounds
    }

    /// 商类成员判定：两个项在当前规则集的闭包内是否同类。
    pub fn equivalent(a: &Term, b: &Term, max_rounds: usize) -> bool {
        let mut eg = EGraph::new();
        let ia = eg.add(a);
        let ib = eg.add(b);
        eg.saturate(max_rounds);
        eg.find(ia) == eg.find(ib)
    }

    /// 图内节点数（测试与体检用）。
    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// 不变量是否已恢复（读操作前置条件；egg 的 `clean` 纪律）。
    pub fn clean(&self) -> bool {
        self.clean
    }

    /// debug 不变量断言（egg `check_memo` 同款纪律，仅 debug_assertions）：
    /// (i) 规范化后的 e-node 全局唯一地属于一个类；(ii) memo 与类表一致。
    #[cfg(debug_assertions)]
    pub fn check_invariants(&mut self) {
        let mut by_enode: BTreeMap<ENode, u32> = BTreeMap::new();
        let roots: Vec<u32> = self.members.keys().copied().collect();
        for root in roots {
            let members: Vec<u32> = self.members[&root].iter().copied().collect();
            for id in members {
                let canon = self.canonicalize(self.nodes[id as usize].clone());
                match by_enode.get(&canon) {
                    Some(&first) => assert_eq!(
                        self.find(first),
                        self.find(id),
                        "canonical enode {:?} lives in two classes",
                        canon
                    ),
                    None => {
                        by_enode.insert(canon, id);
                    }
                }
            }
        }
        let memo_snapshot: Vec<(ENode, u32)> =
            self.memo.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (enode, id) in memo_snapshot {
            // memo 键允许过期（惰性重建语义）；按规范化形态比对。
            let canon = self.canonicalize(enode);
            assert_eq!(
                self.find(id),
                self.find(by_enode[&canon]),
                "memo disagrees with classes for {:?}",
                canon
            );
        }
    }
}

impl ENode {
    /// 遍历子 id（add_enode 登记父表用）。
    fn for_each_child(&self, f: &mut impl FnMut(u32)) {
        match self {
            ENode::Chain(a, b) | ENode::Par(a, b) | ENode::Feedback(a, b) | ENode::Choice(a, b) => {
                f(*a);
                f(*b);
            }
            ENode::Broadcast(a, b, c) | ENode::Merge(a, b, c) => {
                f(*a);
                f(*b);
                f(*c);
            }
            ENode::Diamond(a, b, c, d) => {
                f(*a);
                f(*b);
                f(*c);
                f(*d);
            }
            ENode::Opt(a) => f(*a),
            ENode::Rep(_, a) => f(*a),
            _ => {}
        }
    }
}

// ── 3. 规则集（组合律的具体化）────────────────────────────────────

/// 规则签名：在图上对节点 `node`（规范形态 `e`）尝试一次重写；
/// 返回是否发生了合并。候选项的子位置引用**类 id**（e-node 直接在图上构造）。
type Rule = fn(&mut EGraph, u32, &ENode) -> bool;

/// 当前规则集（v1）：结合律（双向）与单位律（双向）。
///
/// 刻意未收录（依赖类型上下文，无类型标签下不可靠）：对称自然性、
/// 迹方程（vanishing/sliding）、余单位方程。见模块头"诚实边界"。
pub const RULES: &[Rule] = &[
    // 结合律（→）：Chain(Chain(a,b),c) ⇒ Chain(a,Chain(b,c))
    |eg, node, e| {
        let ENode::Chain(l, c) = e else { return false };
        let (l, c) = (*l, *c);
        let members = eg.class_nodes(l);
        for (_mid_id, mid) in members {
            let ENode::Chain(a, b) = mid else { continue };
            let inner = eg.add_enode(ENode::Chain(b, c));
            let new = eg.add_enode(ENode::Chain(a, inner));
            if eg.find(new) != eg.find(node) {
                eg.union(node, new);
                return true;
            }
        }
        false
    },
    // 结合律（←）：Chain(a,Chain(b,c)) ⇒ Chain(Chain(a,b),c)
    |eg, node, e| {
        let ENode::Chain(a, r) = e else { return false };
        let (a, r) = (*a, *r);
        let members = eg.class_nodes(r);
        for (_mid_id, mid) in members {
            let ENode::Chain(b, c) = mid else { continue };
            let outer = eg.add_enode(ENode::Chain(a, b));
            let new = eg.add_enode(ENode::Chain(outer, c));
            if eg.find(new) != eg.find(node) {
                eg.union(node, new);
                return true;
            }
        }
        false
    },
    // 单位律（左）：Chain(Id, a) ⇒ a（Id 经其类根判定——hashcons 唯一节点）
    |eg, node, e| {
        let ENode::Chain(l, a) = e else { return false };
        let (l, a) = (*l, *a);
        let id_node = eg.add_enode(ENode::Id);
        if eg.find(l) == eg.find(id_node) && eg.find(a) != eg.find(node) {
            eg.union(node, a);
            return true;
        }
        false
    },
    // 单位律（右）：Chain(a, Id) ⇒ a
    |eg, node, e| {
        let ENode::Chain(a, r) = e else { return false };
        let (a, r) = (*a, *r);
        let id_node = eg.add_enode(ENode::Id);
        if eg.find(r) == eg.find(id_node) && eg.find(a) != eg.find(node) {
            eg.union(node, a);
            return true;
        }
        false
    },
];

// ── 测试 ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: usize = 64;

    fn inc() -> Term {
        Term::Cell("Inc")
    }
    fn scaler() -> Term {
        Term::Cell("Scaler")
    }

    #[test]
    fn associativity_is_the_quotient() {
        // Chain(Chain(Inc,Scaler),Inc) ≡ Chain(Inc,Chain(Scaler,Inc))（审计 F2 的机器面）。
        let a = Term::chain(Term::chain(inc(), scaler()), inc());
        let b = Term::chain(inc(), Term::chain(scaler(), inc()));
        assert!(EGraph::equivalent(&a, &b, BUDGET));
    }

    #[test]
    fn unit_laws_in_the_quotient() {
        // Chain(Id, a) ≡ a、Chain(a, Id) ≡ a。
        let a = Term::chain(Term::Id, inc());
        let b = Term::chain(inc(), Term::Id);
        let c = inc();
        assert!(EGraph::equivalent(&a, &c, BUDGET));
        assert!(EGraph::equivalent(&b, &c, BUDGET));
        assert!(EGraph::equivalent(&a, &b, BUDGET));
    }

    #[test]
    fn composed_laws_compose() {
        // 嵌套使用两条律：Chain(Chain(Id,Inc),Scaler) ≡ Chain(Inc,Scaler)
        // （左结合→重排→单位消解——多轮饱和的实证）。
        let a = Term::chain(Term::chain(Term::Id, inc()), scaler());
        let b = Term::chain(inc(), scaler());
        assert!(EGraph::equivalent(&a, &b, BUDGET));
    }

    #[test]
    fn non_equivalent_terms_stay_apart() {
        // 负例：原子格不同名 → 不同类（规则不触碰原子）。
        let a = Term::chain(inc(), scaler());
        let b = Term::chain(scaler(), inc());
        assert!(!EGraph::equivalent(&a, &b, BUDGET));
        // 原子与组合不同类。
        assert!(!EGraph::equivalent(&inc(), &Term::chain(inc(), inc()), BUDGET));
    }

    #[test]
    fn saturation_budget_is_respected() {
        // 预算有界：预算为 0 时不饱和，不等保持（假阴性方向，不假阳性）。
        let a = Term::chain(Term::chain(inc(), scaler()), inc());
        let b = Term::chain(inc(), Term::chain(scaler(), inc()));
        assert!(!EGraph::equivalent(&a, &b, 0));
        assert!(EGraph::equivalent(&a, &b, BUDGET));
    }

    #[test]
    fn congruence_propagates_through_union() {
        // 同余：Id ≡ Chain(Id,Id)（单位律），经同余传导，
        // Chain(Id,Inc) ≡ Chain(Chain(Id,Id),Inc)。
        // rebuild 工作表路径：pending 父节点重哈希撞车 → 合并。
        let a = Term::chain(Term::Id, inc());
        let b = Term::chain(Term::chain(Term::Id, Term::Id), inc());
        assert!(EGraph::equivalent(&a, &b, BUDGET));
    }

    #[test]
    fn deep_chain_reassociation() {
        // 工作表在负载下：8 原子深链，全左嵌套 ≡ 全右嵌套。
        // （全图扫描版本此测试即穷；工作表版本近线性。）
        const CELLS: [&'static str; 8] = ["A", "B", "C", "D", "E", "F", "G", "H"];
        let mut a = Term::Cell(CELLS[0]);
        for c in CELLS.iter().skip(1) {
            a = Term::chain(a, Term::Cell(c));
        }
        let mut b = Term::chain(Term::Cell(CELLS[6]), Term::Cell(CELLS[7]));
        for c in CELLS.iter().take(6).rev() {
            b = Term::chain(Term::Cell(c), b);
        }
        assert!(EGraph::equivalent(&a, &b, BUDGET));
    }

    #[test]
    fn egraph_is_deterministic() {
        // 确定性：同一输入两次运行，轮数、图大小与判定一致。
        let a = Term::chain(Term::chain(inc(), scaler()), inc());
        let b = Term::chain(inc(), Term::chain(scaler(), inc()));
        let mut eg1 = EGraph::new();
        let (i1, i2) = (eg1.add(&a), eg1.add(&b));
        let rounds1 = eg1.saturate(BUDGET);
        let mut eg2 = EGraph::new();
        let (j1, j2) = (eg2.add(&a), eg2.add(&b));
        let rounds2 = eg2.saturate(BUDGET);
        assert_eq!(rounds1, rounds2);
        assert_eq!(eg1.size(), eg2.size());
        assert_eq!(
            eg1.find(i1) == eg1.find(i2),
            eg2.find(j1) == eg2.find(j2)
        );
    }

    #[test]
    fn invariants_hold_after_saturation() {
        // debug 不变量断言：饱和后同余与 e-node 唯一性成立（egg check_memo 纪律）。
        let a = Term::chain(Term::chain(Term::Id, inc()), scaler());
        let b = Term::chain(inc(), scaler());
        let mut eg = EGraph::new();
        eg.add(&a);
        eg.add(&b);
        eg.saturate(BUDGET);
        assert!(eg.clean());
        eg.check_invariants();
    }
}
