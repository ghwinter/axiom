# 不完备统一：Lawvere–Yanofsky 不动点、Tarski 真层级与 axiom 分层谱系
# Incompleteness Unification: the Lawvere–Yanofsky Fixed Point, Tarski's Truth Hierarchy, and axiom's Layering

> **性质**：I1 层理论注记（`docs/internal/theory/`，不入 git）。非已实现、非承诺。
> **上游**：[`boundary-ontology.md`](boundary-ontology.md) §2 四轴、§5 三归宿、§9 分层律；
> `docs/en-us/foundations.md` §0 承诺、T3（时序归载体）、§5.8；`docs/en-us/semantics.md` §1。
> 本文把"不可判断、不可表达、定义边界、表达边界、概念边界"统一到对角化/不动点骨架，
> 并给出 axiom 分层谱系的定理级注脚。术语以中英双语并列；二语构成同一概念的唯一指称。
> 记号：Δ₁⁰ 递归集类；T 一致递归可枚举理论；Trueₖ 第 k 层真谓词；A/M/T 代数/机器/类型可表达闭包（同 boundary-ontology）；σ 语义函数；α 对角线映射；Y 对跖对象。

## 摘要 / Abstract

**中文摘要**：本文研究"不可完备化"现象族的统一形式。主要内容：(1) 陈述三个精确形式——递归论的不可完备化（Gödel–Rosser）、Tarski 真不可定义、Lawvere/Yanofsky 不动点定理；并证明三者共享同一对角化骨架但分处不同强度层级（第 1–2 节）；(2) 建立 axiom 边界词汇与数学表述的对应表（Rice→Moore 判定；可定义集;类型闭包 T ⊊ A；真谓词外延→模态④声明）（第 3 节）；(3) 推导分层谱系的设计推论：自指 ⇒ 不完备 ⇒ 抽象层不能以自身语言完备界定物理残差，故 core/runtime/instances 必然构成"隧道式"分层而非递进谱系；三归宿与模态②③④是对不动点定理的工程应答；T6 对拍管理而非消除边界（第 4 节）；(4) 给出诚实边界：本文为既有定理的整理而非新定理，不声称"全量统一"——非自指系统可逃逸对角化但付出表达力代价（第 5 节）。参考文献区分定理级与项目内部级。

**English Abstract**: This note studies the unified form of the incompleteness phenomenon family. Contents: (1) three exact forms are stated—recursion-theoretic incompletability (Gödel–Rosser), Tarski's undefinability of truth, and the Lawvere/Yanofsky fixed-point theorem—and it is shown that the three share one diagonalization skeleton while occupying distinct strength strata (Sections 1–2); (2) a correspondence table maps axiom's boundary vocabulary onto mathematical formulations (Rice→Moore judgment; definable sets; type closure T ⊊ A; truth-predicate externality→modality ④ declaration) (Section 3); (3) design consequences for the layering are derived: self-reference entails incompleteness, hence an abstraction layer cannot delim its physical residue in its own language, so core/runtime/instances necessarily form a "tunnel layering" rather than a progressive spectrum; the three destinations and modalities ②③④ are the engineering response to the fixed-point theorem; T6 cross-checks manage rather than remove the boundary (Section 4); (4) honest boundaries are stated: this note organizes known theorems rather than proving new ones, and it does not claim a total unification—non-self-referential systems can escape diagonalization at the cost of expressive power (Section 5). References distinguish theorem-level from project-internal sources.

**关键词 / Keywords**：不完备；不可判定；真不可定义；不动点定理；对角化；闭包交集；分层谱系；三归宿 / incompleteness; undecidability; undefinability of truth; fixed-point theorem; diagonalization; closure intersection; layering spectrum; three destinations

---

## 0. 引言：不可完备化现象的家族 / Introduction: the Family of Incompleteness Phenomena

工程与理论研讨中反复出现一组同族词汇：**不可判断**、**不可表达**、**定义边界**、**表达边界**、**概念边界**。对应的对象包括：Rice 判定的非平凡语义性质、类型系统无法表达的代数合法对象、系统"真/合法"谓词无法在系统自身内定义。直觉上它们描述"某个无法完备化的概念集合"。本文给出该直觉的现代数学表述：三者分别对应递归论、真谓词理论、范畴论的三个定理，且后者（Lawvere/Yanofsky）是前三者的统一骨架。第 4 节把结果落到 axiom 分层：三层谱系既非任意设计也非必然推演，而是"自指⇒不完备"在工程侧的受迫形态。

## 1. 三种精确形式 / Three Exact Forms

### 1.1 定义边界：递归论的不可完备化 / The Boundary of Definition: Recursion-Theoretic Incompletability

**定理 1.1（Gödel–Rosser）**。对任何包含足够算术的一致递归可枚举理论 T，不存在一致递归可枚举扩展 T′ ⊇ T 使 T′ 完备（对每一句子 φ，T′ ⊢ φ 或 T′ ⊢ ¬φ）。

**Theorem 1.1 (Gödel–Rosser)**. For every consistent r.e. theory T extending a modicum of arithmetic, there is no consistent r.e. extension T′ ⊇ T that is complete (for every sentence φ, T′ ⊢ φ or T′ ⊢ ¬φ).

不得不断言"不可完备化"是集合/理论层面的性质，对应 boundary-ontology 定义 1.2 的定义边界：T 的定理集是 r.e. 的，但"T 的真"不是。这一层是"概念集合不可完备化"的算术层版本。

### 1.2 概念边界：Tarski 真不可定义 / The Conceptual Boundary: Tarski's Undefinability of Truth

**定理 1.2（Tarski）**。对足够强的语言 𝓛，不存在 𝓛 内的公式 True(x) 使对所有句子 φ 成立 True(⌜φ⌝) ↔ φ。

**Theorem 1.2 (Tarski)**. For a sufficiently expressive language 𝓛 there is no 𝓛-formula True(x) satisfying True(⌜φ⌝) ↔ φ for every sentence φ.

系统的"合法性/真"谓词总落在系统表达力之外（须外延至 Tarski 真谓词层级 True₀, True₁, …，或改采其他机制）。这是 axiom 模态④ 声明的数学理由：系统不接受"自己无法验证的命题"为已证明，而是把它显式置于接缝由声明者担责——axiom 的工程对应物是在元层面对"概念边界"的安置。

### 1.3 统一骨架：Lawvere/Yanofsky 不动点定理 / The Unified Skeleton: the Lawvere/Yanofsky Fixed-Point Theorem

**定理 1.3（Lawvere；Yanofsky 统一）**。设 𝓒 为笛卡尔闭范畴，A 为其对象。若存在自映射 α : A → Yᴬ 使对每 a ∈ A 与 y ∈ Y 有 α(a)(a) = y（"A 枚举一切 A→Y 映射"），且 s : Y → Y 无不动点，则矛盾。

**Theorem 1.3 (Lawvere; unified by Yanofsky)**. Let 𝓒 be cartesian closed, A an object. If there is a self-map α : A → Yᴬ with α(a)(a) = y for all a ∈ A, y ∈ Y ("A enumerates all maps A→Y"), and s : Y → Y has no fixed point, then contradiction. ∎

直觉：一个系统若能枚举/谈论关于自身的一切，就能构造出"自身未曾说出的命题"，而它不可能在自身之内。Cantor 对角线、Russell 悖论、Gödel 不完备、Tarski 不可定义、停机问题均是该定理的实例。所有边界词汇——不可判断、不可表达、定义边界、概念边界——是同一不动点定理在一阶逻辑/可计算性/类型论/集合论中的投射。

## 2. 统一与层级差异 / Unification and Strata

**命题 2.1（同一骨架）**。定理 1.1、1.2、1.3 及停机问题共享同一对角化/不动点骨架；任一者可作为其余者的证明模板（Yanofsky 2003 给出范畴论铺陈）。

**Proposition 2.1 (One Skeleton)**. Theorems 1.1, 1.2, 1.3 and the halting problem share one diagonalization/fixed-point skeleton; either can serve as proof template for the others.

**命题 2.2（层级差异，防过度统一）**。会聚于骨架不等于同一强度：停机问题为 Σ₁⁰-完全；Gödel 不完备涉及算术真集 Truth(PA)（超越算术后，位于 Δ¹₁ 之上）；Tarski 真谓词层级是严格递增的强真；Russell/类涉及 von Neumann 层级 V = ⋃Vα，全集非集合。

**Proposition 2.2 (Strata, against over-unification)**. Convergence on the skeleton is not sameness of strength: the halting problem is Σ₁⁰-complete; Gödel incompleteness concerns Truth(PA) (beyond the arithmetic hierarchy, above Δ¹₁); Tarski's truth hierarchy is strictly increasing strong truth; Russell/classes concern the von Neumann hierarchy V = ⋃Vα, the universe being a proper class.

**观察 2.3（"会聚于骨架，分化于层级"）**。axiom 文档以 Rice 支撑"Moore 不可判定"、以闭包交集支撑"三层非递进"——前者是 Σ₁⁰ 层的直接应用，后者是集合论层的非包含陈述，两处引用精确。把二者统一于 Lawvere 是更强的抽象，其书写即本文。

## 3. 词汇对应 / Lexical Correspondence

| axiom 边界词 | 精确形式化 | 出处 |
|---|---|---|
| 不可判断（Moore 判定；Rice） | 非平凡语义性质不可判定（π₂⁰ 完全） | Rice 1953 |
| 定义边界（四轴① definability） | 可定义集 {x : σ(x) ∈ B} 的边界 | 一阶可定义性（Tarski–Vaught） |
| 表达边界（类型闭包 T ⊊ A） | A ∖ T ≠ ∅：代数合法但类型不可表达 | boundary-ontology 定理 3.1 + 三归宿 |
| 概念边界（系统"合法"谓词） | True 不可在系统内定义，须外延层级 | Tarski 1936 |
| 统一根源 | 自指 × 无不动点映射 ⟹ 闭包不可能 | Lawvere 1969 / Yanofsky 2003 |

**推论 3.1（boundary-ontology 的元层重述）**。A（代数闭包）、M（机器闭包）、T（类型可表达闭包）、Obs（观察闭包）互不包含、交点皆真子集——"没有单个闭包能完备覆盖其余"，其范畴论根源即 Lawvere 定理（合法性函子无不动点）。

**Corollary 3.1 (meta-restatement of boundary-ontology)**. The closures A, M, T, Obs are pairwise non-containing with proper intersections—"no single closure completely covers the others"—whose categorical root is the Lawvere theorem (the legality functor has no fixed point).

## 4. 对 axiom 分层谱系的设计推论 / Design Consequences for axiom's Layering

**推论 4.1（隧道式分层必然）**。自指 ⇒ 不完备 ⇒ 抽象层不能以自身语言完备界定其物理残差。故 core（结构/句法可验证层）、runtime（物理语义可声明层）、instances（生态执行接入层）必然是"隧道式"分层——层间存在双向残差（A∖M 与 M∖A），而非单一刻度上的递进谱系。错位感是定理的推论，不是实现缺陷。

**Corollary 4.1 (Necessity of tunnel layering)**. Self-reference entails incompleteness, hence an abstraction layer cannot fully delimit its physical residue in its own language. The three layers therefore necessarily form a tunnel—bidirectional residue (A∖M and M∖A) between layers—rather than a progressive spectrum on one scale. The felt "gap" is a corollary of the theorem, not an implementation defect.

**推论 4.2（接缝 = 残余安置）**。三归宿定理（D1 类型增强 / D2 运行期降级 / D3 显式声明）与模态②③④构成对 Lawvere 定理的工程应答：既然闭包不可能，就把"不可能的那个点"固定为接缝并显式分类安置（声明、见证、对拍），而非对其不作显式安置。（"安置"非"不动点"：与 `Feedback` 的守卫反馈/迹语义无涉——术语卫生，修正 12.10；Lawvere 定理本身的使用不变。）

**Corollary 4.2 (Seams as residue placement)**. The three destinations (D1 type strengthening / D2 runtime degradation / D3 explicit declaration) and modalities ②③④ constitute the engineering answer to the Lawvere theorem: since closure is impossible, the impossible point is pinned as a seam and explicitly classified—declaration, witness, cross-check—rather than pretended away. ("Placement," not "fixpoint": no bearing on `Feedback`'s guarded-feedback/trace semantics—terminological hygiene, amendment 12.10; the use of the Lawvere theorem itself stands.)

**推论 4.3（T6 的角色）**。多物理实现语义等价对拍（T6）管理边界而非消除边界：物理替换的正确性由"同输入同输出"的对拍确立，不声称超越闭包定理的完备性。对拍是定理约束下的最优工程，不是完备性证明。

## 5. 诚实边界 / Honest Boundaries

1. **非新定理**：本文整理既有定理（Gödel/Tarski/Lawvere/Yanofsky），未新增定理级结果；对 axiom 的映射是注释级的对齐，不改变 §4 推论之外的承诺。
2. **不声称全量统一**：Lawvere 骨架要求笛卡尔闭（自指能力）。非自指系统（如线性逻辑的弱化、无不动点的语义域）可阻碍对角化，但以表达力受限为代价——逃逸与能力不可兼得，此为结构事实而非缺陷。
3. **项目内部定位**：与 `boundary-ontology.md`（四轴机制）互补：后者提供边界的分类机制，本文提供边界存在的根源定理。两者均不构成新公理；冲突时以公开规范（`docs/en-us|zh-cn/`）为准。

## 6. 参考文献 / References

**定理级 / Theorem-level**：
- Gödel, K. (1931). Über formal unentscheidbare Sätze der Principia Mathematica und verwandter Systeme I.
- Tarski, A. (1936). Der Wahrheitsbegriff in den formalisierten Sprachen.
- Lawvere, F. W. (1969). Diagonal arguments and cartesian closed categories.
- Yanofsky, N. (2003). A universal approach to self-referential paradoxes, incompleteness and fixed points.
- Rice, H. G. (1953). Classes of recursively enumerable sets and their decision problems.

**项目内部级 / Project-internal**：
- `boundary-ontology.md` §2（四轴）、§5（三归宿）、§9（分层律）、定理 3.1（闭包交集）。
- `docs/en-us/foundations.md` §0（零成本承诺）、T3（时序归物理载体）、§5.8（物理层/抽象层分离）。
- `docs/en-us/semantics.md` §1（core 不重述，runtime 只答"值怎么动"）；`docs/internal/instance-layer-design.md` §2（可替换谓词边界；本地未跟踪工作稿，新 clone 挂空）。