# 元基础：公理放置问题 / Meta-Foundations: The Axiom-Placement Problem

> **性质**：I1 层理论注记（`docs/internal/theory/`，不入 git）。**非已实现、非承诺**。
> **上游**：[`boundary-ontology.md`](boundary-ontology.md) §9 分层律（合法边界 = 可判定合法性 ∪ 显式残余接缝），
> §2 四轴、§5 三归宿、§6 双层信任架构。本文把 §9 之下隐含的元问题显式化并给出其跨学科锚点。
> 术语以中英双语并列，二语构成同一概念的唯一指称，不另设语际映射。
> 记号：C 构成；G 文法区；P 证明区；D 公理区；tₖ 第 k 阶的可判定时间；M 元问题。

## 摘要 / Abstract

**中文摘要**：本文研究构造性学科的基底问题。主要内容：(1) 定义构成 C = (G, P, D) 三分区（文法区/证明区/公理区）并命名元问题 M——"证明的必要性与方式的必然性由什么决定，基底立于何处"；给出 M 的代数核心——D 是 P 的生成元集，M 即基选取问题，并立两条不变量：极小基律与落位律（第 1 节）；(2) 证明回归三难定理：任一正当性论证链终止于无限回退、循环、未证基底三态之一（第 2 节）；(3) 证明证明必要性相对于放置分层（第 3 节），并给出诚实超形式的推论（Tarski/Gödel）；(4) 确立合法性的构成相对性、小内核单一接缝的外审命题、义务源于构成的 Noether 形态（第 4–6 节）；(5) 提出软件形式即公理放置的命题、形式混合命题，与经帕累托修正的构成最优性猜想（含收割侧引理）、迁移诚实与承认规则两命题（第 7–8 节）；(6) 映射回 axiom 与 axiom-semantics：五概念是经批准的宪法词汇，模态 ①②③④ 是认识论强度谱，runtime 是义务代数的机械，诚实纪律是行动式回答 M（第 9 节）。全部命题配有跨学科锚点（逻辑学、递归论、范畴论、哲学、法学、博弈论、物理学、工程系统），并在参考文献中明确区分定理级与哲学表述级。

**English Abstract**: This paper studies the grounding problem of constructive disciplines. Contents: (1) the constitution C = (G, P, D) is defined (grammar / proof / axiom regions) and the meta-question M named—"what determines the necessity of proof and of method, and on what the base stands"; the algebraic core of M is given—D as a generator set of P, M as basis selection—with two invariants: the Minimal-Basis Law and the Placement Law (Section 1); (2) the Regress-Trilemma Theorem: every justification chain terminates in one of three states—infinite regress, circularity, or an unproved base (Section 2); (3) the necessity of proof is proved placement-relative along the decidability ladder (Section 3), with the corollary that honesty is extra-formal (Tarski/Gödel); (4) constitution-relativity of legality, the external-audit proposition for small-kernel single-seam architectures, and the Noether form of obligation-derivation-from-constitution (Sections 4–6); (5) forms as axiom placements, hybrid forms, the Pareto-corrected Constitutional Optimality conjecture (with the Harvest-Side lemma), and two propositions on transition honesty and the rule of recognition (Sections 7–8); (6) mapping back to axiom and axiom-semantics: the five concepts are a ratified constitutional vocabulary, modalities ①②③④ are the epistemic-strength spectrum, the runtime is the machinery of the obligation algebra, and the honesty discipline is M answered in action (Section 9). Every proposition carries interdisciplinary anchors (logic, recursion theory, category theory, philosophy, law, game theory, physics, engineering systems), with a clear theorem-level vs philosophical-level distinction in the references.

**关键词 / Keywords**：基底问题；构成；公理区放置；回归三难；证明分层；诚实规则；义务代数；识别词 / grounding problem; constitution; axiom placement; regress trilemma; stratified proof; honesty rule; obligation algebra; rule of recognition

---

## 0. 引言：元问题 M / Introduction: the Meta-Question M

**中文**：对任何构造性学科，存在两类必然性追问：(a) "该性质**必须被证明**吗"；(b) "学科**必须如此组织**吗"。此类追问不能在该学科内部获得最终答案——对 (a) 的证明链沿可判定性上行必达不可判定段，对 (b) 的组织论证下行必达未证基底。本文把 a/b 的共同根系命名为**元问题 M**：*一个构造性学科把合法性建立分为三区（文法区、证明区、公理区），三区的划分由谁决定，划分本身立于什么之上。* 第 1 节给出构成的形式定义与 M 的代数核心（基选取、极小基律、落位律）；第 2–6 节以定理与命题形式陈述 M 的子命题并附跨学科锚点；第 7–8 节给出软件形式与最优性问题；第 9 节映射回 axiom。

**English**: For any constructive discipline there are two questions of necessity: (a) "must this property be proved"; (b) "must the discipline be organized this way". Neither admits a final answer inside the discipline: chains of justification for (a) ascend the decidability ladder into the undecidable; arguments for (b) descend into an unproved base. This paper names the common root of a/b the meta-question M: *a constructive discipline divides the establishment of legality into three regions (grammar, proof, axiom); who decides the division, and on what does the division itself stand.* Section 1 formalizes the constitution and gives the algebraic core of M (basis selection, Minimal-Basis Law, Placement Law); Sections 2–6 state the sub-propositions of M with interdisciplinary anchors; Sections 7–8 treat software forms, hybridity, and optimality; Section 9 maps back to axiom.

---

## 1. 记号与基本定义 / Notation and Basic Definitions

**定义 1.1（构成 / Constitution）**。构成是三元组 C = (G, P, D)：G 为文法区（构造拒绝，良构即合法，存在即证明）；P 为证明区（在各自可判定时间 tₖ 可判定的谓词及其证据）；D 为公理区（被声明、不被证明的基底）。

**Definition 1.1 (Constitution)**. A constitution is a triple C = (G, P, D): G the grammar region (construction refusal; well-formedness is legality; existence is proof); P the proof region (predicates decidable at their times tₖ, with evidence); D the axiom region (the declared, unproved base).

**定义 1.2（文法区 / Grammar Region）**。G 由良构条件组成：违反者不可构造，无独立检查步骤（构造拒绝，boundary-ontology 定义 9.1 的 Illegibility）。

**定义 1.3（证明区 / Proof Region）**。P 由谓词及其证据组成：每个 p ∈ P 在某一 tₖ 可判定，其证据按强度递减为模态①（结构见证：类型级配对，如 `Conforms<Wire>`）、模态②（常量见证）、模态③（部署期验证）。G 与①的分界：G 的违反无独立见证对象（构造本身即失败）；凡存在可引用见证对象的编译期判定归 P。

**Definition 1.3 (Proof Region)**. P consists of predicates together with their evidence: each p ∈ P is decidable at some time tₖ, with evidence of decreasing strength—modality ① (structural witness: type-level pairing, e.g. `Conforms<Wire>`), modality ② (constant witness), modality ③ (deployment-time validation). Boundary between G and ①: a violation of G has no separate witness object (construction itself fails); any compile-time decision with a citable witness object belongs to P.

**定义 1.4（公理区 / Axiom Region）**。D 由被声明为真的命题组成：其真值不由体系验证（模态④声明；Rice 区与无理由区）。D 二分：**逻辑-D**（文法、概念封闭、全函数性——以连贯性与使用检验）与**经验-D**（硬件时序、网络可用性、性能预算——以量化界与持续监测检验）。两类展出义务不同：逻辑-D 展出为假定；经验-D 展出为界值与监测手段。

**Definition 1.4 (Axiom Region)**. D consists of propositions declared true whose truth the system does not verify (modality ④ declaration; the Rice region and the reason-free region). D splits into **logical-D** (grammar, concept closure, totality—checked by coherence and use) and **empirical-D** (hardware timing, network availability, performance budgets—checked by quantified bounds plus continuous monitoring). Their exhibition duties differ: logical-D is exhibited as assumption; empirical-D as bounds and monitoring.

**定义 1.5（诚实规则 / Honesty Rule）**。D 的每一成员必须被展出，不得伪装为 P 的成员；"看起来已验证的声明"违反规则（boundary-ontology 注记 6.2 的声明≠证明）。

**Definition 1.5 (Honesty Rule)**. Every member of D must be exhibited and must not be disguised as a member of P; "a declaration that looks verified" violates the rule.

**定义 1.6（义务类 / Obligation Class）**。义务类是物理层（或任何非代数层）谓词的参数化族：投递态（Full/Closed/Timeout/Cancelled）× 资源类（零分配/每消息/外部）× 引用有效（代戳）× 生命周期（许可阶段）；每条接缝声明其义务类，装配点按模态③ 校验。

**Definition 1.6 (Obligation Class)**. An obligation class is a parameterized family of physical-layer (or any non-algebraic layer) predicates: delivery state (Full/Closed/Timeout/Cancelled) × resource class (zero-alloc / per-message / external) × reference validity (generation stamps) × lifecycle (license phases); each seam declares its obligation class, and assembly points validate it at modality ③.

**定义 1.7（公理放置 / Axiom Placement）**。公理放置是从软件形式 F 到构成 C(F) 的映射：F ↦ (G(F), P(F), D(F))；不同形式差别仅在 D 与 P 的相对边界位置。

**Definition 1.7 (Axiom Placement)**. An axiom placement is a mapping from a software form F to a constitution C(F): F ↦ (G(F), P(F), D(F)); forms differ only in the relative boundary between D and P.

**定义 1.8（基与生成元 / Basis and Generators）**。固定推理规则集 R，则 P 是 G ∪ D 在 R 下的闭包；D 是 P 相对 R 的生成元集。公理放置问题的代数形式即**基选取**：基不唯一（等价基交换＝一次重构），故诚实的抱负不是唯一性，而是下两条不变量。

**Definition 1.8 (Basis and Generators)**. Fixing an inference-rule set R, P is the closure of G ∪ D under R; D is a generator set of P relative to R. The algebraic form of the axiom-placement problem is **basis selection**: bases are not unique (an equivalent-basis swap is a refoundation), so the honest ambition is not uniqueness but the two invariants below.

**定义 1.9（极小基律 / Minimal-Basis Law）**。D 中不得含有可由其余成员加 R 导出的成员。违例＝伪验证类缺陷（把可证项冒充为公设）。执行程序：审计与"声明 ≠ 证明"标注。

**Definition 1.9 (Minimal-Basis Law)**. No member of D may be derivable from the remaining members plus R. Violation = the pseudo-verification defect (a provable item disguised as a postulate). Enforcement: audits and declaration≠proof labeling.

**定义 1.10（落位律 / Placement Law）**。每条义务必须置于其见证形态所能支撑的最强模态。放弱＝浪费可判定性（保证降级）；放强＝伪精确（不可判者硬塞早期模态，被迫退化为纯声明）。实例：Moore 内联环安全是层域谓词但 Rice 不可判定 ⟹ 只可④；容量非零有常量见证 ⟹ 必须②。

**Definition 1.10 (Placement Law)**. Every obligation must be placed at the strongest modality its witness form supports. Too weak wastes decidability (degraded guarantee); too strong is false precision (the undecidable forced into an early modality degenerates into bare declaration). Instances: inline-loop Moore safety is layer-domain yet Rice-undecidable ⟹ ④ only; capacity-nonzero has a constant witness ⟹ ② required.

**定义 1.11（元问题 M / the Meta-Question M）**。M：构成的三分区划分由什么决定，划分本身立于什么之上；以及 a/b 两类必然性追问在何层获得何种答案。

**Definition 1.11 (the Meta-Question M)**. M: what determines the tri-partition of a constitution, and on what does the partition itself stand; and at which stratum, with which answer type, the two necessity questions a/b are settled.

---

## 2. 回归三难定理 / The Regress-Trilemma Theorem

**定理 2.1（回归三难 / Regress Trilemma）**。设 C = (G, P, D) 为一构成，π 为其中任一正当性论证（对某声明 s ∈ D ∪ P 的支持链）。则 π 恰终止于三态之一：(T₁) 无限回退；(T₂) 循环；(T₃) 未证基底（s 被声明为 D 的成员）。三态不可逃逸。

**Theorem 2.1 (Regress Trilemma)**. Let C = (G, P, D) be a constitution and π any justification argument (a support chain for a claim s ∈ D ∪ P). Then π terminates in exactly one of three states: (T₁) infinite regress; (T₂) circularity; (T₃) an unproved base (s declared a member of D). The three states are inescapable.

**证明**。s 的支持关系给出一条上行链。沿支持边自 s 上行：(i) 若存在各项两两不同的无穷上升链，终止于 (T₁)；(ii) 否则链有限。若末项无前件，则其为被声明项，支撑止于 D，终止于 (T₃)；若上行途中回到已访问项，支持关系闭合成环，终止于 (T₂)。三态互斥：T₁ 无末项，T₂ 无基底，T₃ 无回退。∎

**Proof.** The support relation on s induces an upward chain. Ascending from s along support edges: (i) if there exists an infinite ascending chain with pairwise distinct terms, π terminates at (T₁); (ii) otherwise the chain is finite. If the last term has no antecedent, it is a declared item and support terminates in D—(T₃); if ascent revisits a visited term, the support relation closes into a cycle—(T₂). The three states are mutually exclusive: T₁ has no last term, T₂ no base, T₃ no regress. ∎

**推论 2.2（基底不可由内部证明 / The Base Cannot Prove Itself）**。不存在构成 C 使 D ⊆ P 或使 C 在自身内部证明其诚实：自指一致性与真之不可定义（Gödel 1931；Tarski 1936）排除之。锚点：Gödel II；Tarski 真之不可定义性。

**Corollary 2.2 (The Base Cannot Prove Itself)**. No constitution C admits D ⊆ P, nor can C prove its own honesty internally: self-referential consistency and the undefinability of truth exclude it (Gödel 1931; Tarski 1936).

**命题 2.3（基底只可展出 / The Base Can Only Be Exhibited）**。公理区的合法化方式是展出（声明 + 标注），不是证明；该展出法则自身是方法论约定（Popper 1934 的可证伪性即约定；Kelsen 1934 的 Grundnorm 即预设）。

**Proposition 2.3 (The Base Can Only Be Exhibited)**. The legitimization of the axiom region is exhibition (declaration plus labeling), not proof; the exhibition law itself is a methodological convention (Popper 1934: falsifiability is a convention; Kelsen 1934: the Grundnorm is a presupposition).

**注记 2.4**。三难的历史与哲学锚点：Agrippa（经 Sextus Empiricus 的《皮浪主义纲要》流传的五式/无判据三难）；Albert 1968 的 Münchhausen 三难（无限回退/逻辑循环/教条终止）；Sellars 1956 的"所予的神话"（直接给予的基底地位）。

---

## 3. 证明必要性分层 / Stratified Necessity of Proof

**定理 3.1（证明必要性分层 / Stratified Necessity of Proof）**。对构成 C = (G, P, D) 的任一性质 q，必要性相对于放置成立：(i) 若 q 已被放置于某层且在 tₖ 可判定，则该层的证据（模态①②③之一）为强制——放弃判定即产生静默残余，违反定理 9.6 之 (L₂)；(ii) 若 q 不可判定，其证据只能是声明（模态④）；(iii) 在塔顶（G 的良构性），不存在证据概念——合法由构造本身确立。可判定性只提供选项；义务来自放置决定；放置本身受定义 1.10 约束但属构成选择。

**Theorem 3.1 (Stratified Necessity of Proof)**. For any property q of a constitution C = (G, P, D), necessity is placement-relative: (i) if q has been placed at a stratum and is decidable at tₖ, evidence at that stratum (one of modalities ①②③) is mandatory—forgoing the decision creates a silent residual, violating (L₂) of Theorem 9.6; (ii) if q is undecidable, its evidence can only be a declaration (modality ④); (iii) at the top (well-formedness of G) no notion of evidence exists—legality is established by construction itself. Decidability provides options; obligations come from placement decisions; placement itself is constrained by Definition 1.10 yet remains a constitutive choice.

**证明**。(i) 由落位律（定义 1.10），q 既已置于该层，其见证形态在 tₖ 可判定；不执行判定则该层边界出现静默残余——由定理 9.6 之 (L₂)，残余必须被显式承载，静默即违例。(ii) 由 Rice 1953，无一般判定程序；任何声称的证据必为外部假设，即声明。(iii) 同原证：G 的良构性由文法的存在本身确立。∎

**Proof.** (i) By the Placement Law (Definition 1.10), once q is placed at the stratum its witness form is decidable at tₖ; forgoing the decision leaves a silent residual at the stratum boundary—which Theorem 9.6 (L₂) requires to be carried explicitly, so silence is the violation. (ii) By Rice 1953 no general decision procedure exists; any claimed evidence is an external hypothesis, i.e., a declaration. (iii) As before: well-formedness of G is established by the existence of the grammar itself. ∎

**推论 3.2（诚实是超形式的 / Honesty Is Extra-Formal）**。构成 C 不能证明自身的诚实性；诚实规则的执行者位于 C 之外（审计、文档纪律、外部读者）。锚点：Tarski 1936（真之不可定义）；Gödel II（一致性不可内部证明）；Lakatos 1963-64（隐藏引理的暴露是增长机制）；Popper 1934。

**Corollary 3.2 (Honesty Is Extra-Formal)**. A constitution C cannot prove its own honesty; the enforcers of the honesty rule lie outside C (audit, documentation discipline, external readers). Anchors: Tarski 1936; Gödel II; Lakatos 1963-64 (exposure of hidden lemmas as a growth mechanism); Popper 1934.

**命题 3.3（证明强度谱 / Spectrum of Proof Strength）**。证明必要性沿递升谱系组织：算术分层的复杂度阶（Kleene 1943；Post），逆向数学中的子系统强度（Friedman/Simpson 1999：定理按所需公理分类），序数分析中的证明强度（Gentzen 1936）。此谱系是定理 3.1 的度量版本。

**Proposition 3.3 (Spectrum of Proof Strength)**. Necessity of proof is organized along ascending spectra: the complexity tiers of the arithmetical hierarchy (Kleene 1943; Post), the subsystem strengths of reverse mathematics (Friedman/Simpson 1999: theorems classified by the axioms they require), and the ordinal measures of proof strength (Gentzen 1936). These spectra are the metric version of Theorem 3.1.

---

## 4. 合法性的构成相对性 / Constitution-Relativity of Legality

**命题 4.1（相对性 / Relativity）**。软件的"合法"不是一元谓词，而是相对于构成 C 的关系：同一对象在不同构成下合法与否可不同。锚点：boundary-ontology §8 结论 (1) 的上行推广；逆向数学中"真理性随基底"。

**Proposition 4.1 (Relativity)**. Software "lawfulness" is not a unary predicate but a relation relative to a constitution C: the same object may be lawful under one constitution and unlawful under another.

**命题 4.2（topos 形态 / The Topos Form）**。每个构成 C 定义一个"可在此构造的世界"；不同软件形式是不同世界。锚点：Lawvere 的 ETCS（1964-66）把集合论基础化为一个范畴公理系统；topos 理论中每个 topos 是一个可做数学的宇宙；定理 2.2/3.1 在世界间不迁移（每世界有自己的可判定边界）。

**Proposition 4.2 (The Topos Form)**. Each constitution C defines a "world in which construction is possible"; different software forms are different worlds. Anchors: Lawvere's ETCS (1964–66); in topos theory each topos is a universe of mathematics; Theorems 2.2/3.1 do not migrate between worlds (each world has its own decidability boundary).

**命题 4.3（类型论形态 / The Type-Theoretic Form）**。文法区由构造拒绝实现：良类型程序不会出错（Milner 1978）；Curry-Howard 同构使证明区与文法区共享同一代数（类型即命题，证明即程序）。axiom 实例：`Conforms`/`Wire` 的 T1 配对即文法区的类型论执行。

**Proposition 4.3 (The Type-Theoretic Form)**. The grammar region is realized by construction refusal: well-typed programs cannot go wrong (Milner 1978); the Curry–Howard isomorphism gives the proof region and the grammar region a shared algebra (types as propositions, proofs as programs). axiom instance: the T1 pairing of `Conforms`/`Wire` is the type-theoretic execution of the grammar region.

---

## 5. 小内核与单一接缝 / Small Kernel and the Single Seam

**命题 5.1（外审命题 / The External-Audit Proposition）**。任何双层信任架构 (K, J)（boundary-ontology 定义 6.1）的可靠性不能由 K 自我证明；可靠性论证必含外部审查步骤。锚点：de Bruijn 判据（证明检查器须小到可人工审查）；seL4（Klein et al. 2009：微内核 + 机器检查证明，其证明工具链在 TCB 之外）；LCF（Gordon/Milner/Wadsworth 1979：扩展无法穿透内核可靠性）；Hoare 1981（《皇帝的旧衣》：可靠性以简单性为前提，两层软件的成本由用户承担）。

**Proposition 5.1 (The External-Audit Proposition)**. The reliability of any two-layer trust architecture (K, J) (boundary-ontology Definition 6.1) cannot be proved by K itself; any reliability argument contains an external audit step. Anchors: the de Bruijn criterion (a proof checker must be small enough to audit by hand); seL4 (Klein et al. 2009: microkernel plus machine-checked proof, whose toolchain lies outside the TCB); LCF (Gordon/Milner/Wadsworth 1979: extensions cannot breach kernel soundness); Hoare 1981 (reliability presupposes simplicity; the cost of complexity is paid by users).

**命题 5.2（fail-closed 默认 / Fail-Closed Defaults）**。构造级公理：许可与能力的默认态为拒绝。锚点：Saltzer & Schroeder 1975（fail-safe defaults、least privilege、separation of privilege）；现代实据：claude-code-main 的 `buildTool` 以并发安全=否、只读=否作为默认（挖掘所得实物证据）。axiom 实例：`CarrierCost` 默认 `External`（未声明不视为零分配）。

**Proposition 5.2 (Fail-Closed Defaults)**. Constitution-level axiom: the default state of permissions and capabilities is denial. Anchors: Saltzer & Schroeder 1975 (fail-safe defaults, least privilege, separation of privilege); modern evidence: claude-code-main's `buildTool` defaults concurrency-safety and read-only to false (mined artifact); axiom instance: `CarrierCost` defaults to `External` (undeclared is not zero-allocation).

---

## 6. 义务源于构成 / Obligations Derive from Constitution

**命题 6.1（Noether 形态 / The Noether Form）**。对称性假设 ⇒ 守恒义务；构成选择 ⇒ 规范义务。数学形态（已证）：Noether 1918（守恒律源于对称性）；规范原理（Yang & Mills 1954：要求局部规范不变性决定相互作用内容——"公理放置决定内容"的物理学标准实例）。构造形态（构成性表述，非可证定理）：义务类语法（投递态 × 资源类 × 引用有效 × 生命周期）由构成规则导出；**断言其与 Noether 形态同构，而非声称其可证明**。标注纪律的自用：本条整体为命题级——数学内核引用已证定理，工程映射为定义后果，故不冠"定理"。

**Proposition 6.1 (The Noether Form)**. Symmetry assumptions imply conservation obligations; constitution choices imply normative obligations. Mathematical form (proved): Noether 1918 (conservation from symmetry); the gauge principle (Yang & Mills 1954: requiring local gauge invariance determines the interaction content—physics' canonical instance of "axiom placement determines content"). Constructive form (constitutional statement, not a provable theorem): the obligation-class grammar (delivery state × resource class × reference validity × lifecycle) is derived from the constitution rules; the claim is isomorphism with the Noether form, not provability. Labeling discipline applied to ourselves: this item is proposition-level overall—the mathematical core cites proved theorems, the engineering mapping is a consequence of definitions, hence no "theorem" heading.

**注记 6.2（类比边界 / Boundary of the Analogy）**。Noether 定理是数学定理；构成 ⇒ 义务在工程层是构成性表述（定义的后果，受定义约束），不获独立证明。类比的价值在结构（对称/构成 → 守恒/义务的派生通道），不在证明力。

**Remark 6.2 (Boundary of the Analogy)**. Noether's theorem is a mathematical theorem; constitution ⇒ obligation is a constitutional statement at the engineering layer (a consequence of definitions, bounded by them), without independent proof. The analogy's value is structural (the derivation channel symmetry/constitution → conservation/obligation), not its proof power.

---

## 7. 软件形式的公理放置 / Axiom Placement across Software Forms

**命题 7.1（形式即放置 / Forms Are Placements）**。任意软件形式 F（工具、服务、服务器、内核、游戏、交互体）对应构成 C(F)，差别 = 公理区放置：

| 形式 F | 公理区 D(F) 的典型内容 | 证明区 P(F) 的密度 | 义务类 |
|---|---|---|---|
| kernel | 硬件（中断、存储层次、确定性调度） | 小而密（可判定时间紧） | 零分配、代戳句柄、静态池 |
| service/server | 环境（网络、可用性、第三方） | 中（投递四态、超时、重试可判） | 每消息分配、投递态、超时 |
| tool/CLI | 用户意图、会话语义 | 稀 | 轻（会话 FSM、输入接缝） |
| game | 体验语义本身 | 极稀（残余容忍高） | 时效、状态一致性 |

**Proposition 7.1 (Forms Are Placements)**. Every software form F (tool, service, server, kernel, game, interactive body) corresponds to a constitution C(F); the difference is the axiom placement (see table).

> **表注**：kernel 行的"硬件"、service 行的"环境"属经验-D（定义 1.4）——不是逻辑起点，而是带量化界的假设；其诚实义务为界值与监测手段（SLA、遥测、压测），非逻辑连贯性。
>
> **Table note**: "hardware" and "environment" in the table are empirical-D (Definition 1.4)—not logical starting points but bounded assumptions; their honesty duty is bounds and monitoring (SLAs, telemetry, load tests), not logical coherence.

**注记 7.1（图范式覆盖 / Coverage of the Graph Paradigm）**。"节点 + 边"图是计算机科学最通用的描述与驱动范式，分布于十个领域：深度学习计算图、流处理数据流图（Beam/Flink/Ray）、工作流 DAG（Airflow/Temporal）、ECS＋场景图、渲染图（Scene/Frame Graph、wgpu）、构建依赖图（Cargo/Make/Bazel/Ninja）、响应式信号图（Dioxus/SolidJS/egui）、Actor/服务调用图、区块链状态机、编译器 IR（CFG/DFG/调用图）。按**边的语义**三分，其与 axiom 的关系各异：

| 类别 | 边语义 | 代表 | 与 axiom 的关系 |
|---|---|---|---|
| (1) | 值流（因果数据流） | 深度学习静态计算图、流处理、构建依赖图、渲染图、Actor 邮箱、区块链状态机、基本块内数据流 | **原生**：节点 = PortCell，边 = Wire，拓扑 = 组合子；扇出/扇入 = `Broadcast`/`Merge`；背压 = 物理层有界载体；增量构建缓存 = `Seat` 代戳（引用有效性）；控制流 = `Choice`/`Opt`/`Feedback`（控制编码为值） |
| (2) | 控制/时序依赖 | 工作流 DAG、GPU barrier、ECS 系统调度 | **编码或物理化**：完成依赖编码为令牌值流；隐式布线仲裁归物理层调度器；World 共享状态违反 M1，经载体物理化 |
| (3) | 非因果约束/隐式关系 | 反向传播（梯度为沿边反向的第二流）、电路/多体仿真（方程组）、响应式隐式订阅 | **axiom 边界**：落入三归宿（接缝声明/降级编码/类型强化）与分层律（L₂）的接缝位置 |

结论：所有图范式共享同一 M 结构（开放系统 = cell、布线 = wiring、宿主 = 载体、组合 = 组合子），差异只在公理区放置与物理载体选择——这是命题 7.1（形式即放置）与 theory-archive.md §1.2（三种组织策略："开放系统 + 布线 + 实例 + 组合，仅布线隐/显、宿主形态、组合打包不同"）的更大跨度实例。axiom 是**因果数据流图的最小封闭内核（五概念，§8.3）+ 物理层载体市场**，不是一切图的元模型。

**Remark 7.1 (Coverage of the Graph Paradigm).** The node-and-edge graph is the most general description-and-driving paradigm in computing, instantiated across ten domains: DL computation graphs, streaming dataflow graphs (Beam/Flink/Ray), workflow DAGs (Airflow/Temporal), ECS + scene graphs, render graphs (Scene/Frame Graph, wgpu), build dependency graphs (Cargo/Make/Bazel/Ninja), reactive signal graphs (Dioxus/SolidJS/egui), actor/service call graphs, blockchain state machines, and compiler IR (CFG/DFG/call graphs). Classified by **edge semantics**, three kinds differ in their relation to axiom:

| Class | Edge semantics | Representatives | Relation to axiom |
|---|---|---|---|
| (1) | Value flow (causal dataflow) | DL static graphs, streaming, build graphs, render graphs, actor mailboxes, blockchain state machines, intra-block dataflow | **Native**: a node is a PortCell, an edge is a Wire, topology is the combinators; fan-out/fan-in are `Broadcast`/`Merge`; backpressure is a bounded physical carrier; incremental caching is `Seat`-generation reference validity; control flow is `Choice`/`Opt`/`Feedback` (control encoded as values) |
| (2) | Control/timing dependencies | Workflow DAGs, GPU barriers, ECS system scheduling | **Encoded or physicalized**: completion dependencies become token-valued flows; implicit wiring arbitration belongs to the physical scheduler; shared World state violates M1 and is physicalized through carriers |
| (3) | Non-causal constraints / implicit relations | Backpropagation (gradients: a second reverse flow), circuit/multi-body simulation (equation systems), reactive implicit subscription | **Axiom boundary**: falls into the three settlements (seam admission / encoding degradation / type strengthening) and the stratification-law (L₂) seam |

All graph paradigms share the same M-structure (open system = cell, wiring, host = carrier, composition = combinators); differences are only axiom placement and physical carrier choice—a larger instantiation of Prop. 7.1 (forms are placements) and of the archived theory-archive.md §1.2 conclusion (three organization strategies: "open system + wiring + instance + composition, differing in implicit/explicit wiring, host form, and composition packaging"). Axiom is the **minimal closed kernel of causal dataflow graphs (five concepts, §8.3) plus a physical-layer carrier market**, not a meta-model of all graphs.

**命题 7.2（普遍性 / Universality）**。所有软件形式共享同一 M 结构；参数 = 公理区放置。义务代数是构成的语法，M 是构成的语义：每条接缝的声明义务 + 装配校验，是把"该系统的公理区放在何处"写成可检查句子。锚点：topos 宇宙（命题 4.2）；Lamport 2002（TLA+：规约语言即构成语言）；逆向数学（基底选择可度量）。

**Proposition 7.2 (Universality)**. All software forms share the same M-structure; the parameter is the axiom placement. The obligation algebra is the syntax of constitutions; M is their semantics: the declared obligation plus assembly validation of each seam writes "where this system's axiom region lies" as a checkable sentence.

**命题 7.3（形式混合 / Hybrid Forms）**。真实系统是多形式杂交体：同一项目内嵌多个 C(F)，M 在形式之间的接缝处逐段重新提问。实据：Linux 内核将确定性实时内容（SCHED_FIFO/SCHED_RR 实时调度类）与通用任务调度同驻于单一内核——实时孤岛是通用宿主中的 kernel 行内容；应用侧同理，确定性驱动（固定步长模拟、音频实时回调）嵌于通用应用宿主，构成 tool 行世界中的 kernel 行孤岛。

**Proposition 7.3 (Hybrid Forms)**. Real systems are multi-form hybrids: one project embeds several C(F), and M re-arises per boundary between forms. Evidence: the Linux kernel hosts deterministic real-time content (the SCHED_FIFO/SCHED_RR real-time scheduling classes) alongside general task scheduling in a single kernel—real-time islands are kernel-row content in a general host; likewise on the application side, deterministic drivers (fixed-step simulation, real-time audio callbacks) embedded in a general application host constitute kernel-row islands in a tool-row world.

**命题 7.4（实现域的标准化体系 / Standardization of Implementations）**。实例实现的标准化在业界已成熟，其普遍解剖为六元组 (S, L, T, C, V, R)：

- **S 接口与可观察行为契约**（机制自由、表面受约）＝型位许可面的工业形态；
- **L 规范强度语言**（RFC 2119/8174 的 MUST/SHOULD/MAY）＝义务论轴，与认识论模态 ①②③④ **正交**：MUST 只规定义务绑定，其卸载仍须按落位律（定义 1.10）指派模态——写成符合性测试（③）或仅作文本声明（④）皆可；SHOULD 的"偏离须附文档化理由"条款即诚实规则的标准语形态；MAY 即放置自由；
- **T 符合性测试与认证**（POSIX VSX、USB-IF、Java TCK、WASM 符合性套件）＝外审命题（5.1）的机制化：实现者不自证；
- **C 剖面/等级**（POSIX 1003.13 PSE51–54、ISO 26262 ASIL A–D、DO-178C DAL A–E、RISC-V RVA20/22）＝公理区放置（命题 7.1）的标准化版本：实现类别即放置类别；
- **V 版本化**（POSIX 版次、bcdUSB、semver）＝生态跨时间互操作的代计数——与句柄生成戳**同形不同效**：前者防混认于生态时间，后者防陈旧引用于系统生存期（类比边界依注记 6.2 方式标定）；
- **R 治理与修订程序**（IETF rough consensus、ISO 委员会、USB-IF 董事会）＝承认规则（命题 8.5）的工业形态；五元组若无 R 则 V 无引擎——版本由谁决定？

最佳完整实例：WebAssembly（规范正文＋形式操作语义＋参考解释器＋符合性套件＋核心/扩展分层——P 区达①级形态语义）；OSI 服务原语四元组（REQUEST/INDICATION/RESPONSE/CONFIRMATION，1984）是投递态分类学的先驱形态。反例张力：Rust 长期以 rustc 为事实规范——de Bruijn 判据未满足（检查器不小），Ferrocene 语言规范是补位尝试。MISRA C:2012 是文法区的标准化（对不安全语言施加可检查子集）；ARINC 653 与 ISO 26262 是义务类的标准化。推论：约束经六个通道作用于实现域——表面契约、强度分级、外部认证、剖面子集、版本代计、治理程序；独立发明收敛到同一结构，是构成理论的经验证据。

**Proposition 7.4 (Standardization of Implementations)**. Implementation standardization is mature industrial practice; its universal anatomy is the six-tuple (S, L, T, C, V, R): S interface-and-observable-behavior contracts (mechanism-free, surface-bound) = the industrial form of the slot-license surface; L normative-strength language (RFC 2119/8174 MUST/SHOULD/MAY) = the deontic axis, orthogonal to the epistemic modalities ①②③④—MUST only binds the obligation, whose discharge still requires a modality assignment per the Placement Law (Definition 1.10): a conformance test (③) or a textual declaration (④); SHOULD's documented-deviation clause is the honesty rule in standards language; MAY is placement freedom. T conformance testing and certification = mechanized external audit (Proposition 5.1). C profiles/levels (PSE51–54, ASIL A–D, DAL A–E, RVA20/22) = standardized axiom placements (Proposition 7.1). V versioning = generation-counting for ecosystem-time interoperability—same shape as handle stamps, different effect (mixing across ecosystem time vs stale references within system lifetime; analogy bounded per Remark 6.2 style). R governance/amendment procedure (IETF rough consensus, ISO committees, USB-IF board) = the industrial form of the rule of recognition (Proposition 8.5); without R, V has no engine. Best complete instance: WebAssembly; OSI service primitives (1984) prefigure the delivery-state taxonomy; Rust-as-rustc is the running tension against the de Bruijn criterion (Ferrocene FLS as remedy). Corollary: constraints act on implementations through six channels—independent inventions converging on one structure is empirical evidence for constitution theory.

**命题 7.5（系统间接缝与契约型位 / Inter-System Seams and Contractual Slots）**。跨系统交互面（设备树、插件系统、客户端-服务端、跨机器微服务）是同一结构的四种绑定形态：

- **契约型位（contractual slot）**：交互面在编译期以"类型 = 契约"存在（外部系统的签名：消息类型、方向、义务类、模态），实现在未来/运行期被安装；外部系统**不在编译图中**——所在构成独立编译，不编译对方实现。参数 = **绑定时刻 × 绑定机制**：设备树（构建期编译 + 运行期枚举，match 表配对）、插件系统（运行期加载，接口注册）、客户端-服务端（连接建立，地址 + 协议握手）、微服务（网络载体，契约 + 投递态）。
- **可判定性分界**：凡"关于已存在之物"的相合性 → 可判（证明性：契约相合 ①③、自身实现内部性质 ①②③）；凡"关于未来实现或外部语义"的行为 → 不可判（编译期），只能采样验证（运行期测试）或诚实声明（④）＋监测（经验-D）。
- **占位角色**：占位（stub/mock）使宿主内集成在编译期闭合——它验证**契约-宿主相合**，不验证**真实端-宿主相合**（占位通过 ⊬ 真实端通过）；真实端的行为等价属运行期（T6 类）。
- **可判定性全景**：

| 验证对象 | 判定性 | 执行时刻 |
|---|---|---|
| 契约相合（型位 ↔ 契约） | 可判（②③） | 编译期/装配 |
| 自身实现内部性质（组合/全函数/义务） | 可判（①②③） | 编译期/装配 |
| 占位/真实端与契约相合（实现存在时） | 可判 | 构建期 |
| 跨端行为等价 | 采样验证 | 运行期 |
| 未来实现的行为正确性 | 不可判 → ④＋监测 | 未来/运行期 |
| 交互序列语义（时序/断连/延迟） | 经验-D（界＋监测） | 运行期 |

- **理论落位**：这是概念 4（型位）的最大尺度应用、系统间接缝命题（多个构成经系统间载体互连、契约相合 ∀ 编译期验证、存在 ∃ 运行期绑定、缺失 = 投递态 Closed）；也是命题 7.1/7.3（形式即放置/形式混合）的跨项目形式。工业收敛实例：Linux 设备模型（驱动-设备配对）、FIDL/协议 schema（protobuf/OpenAPI）、consumer-driven contract testing（Pact）、FFI/ABI 边界——均为同一结构的特化；axiom 给出其最小内核（型位 + 四模态 + 义务代数），并借分层律（定理 9.6）把不可判部分定位到接缝（L₂）而非隐藏。

**Proposition 7.5 (Inter-System Seams and Contractual Slots)**. Cross-system interaction surfaces (device trees, plugin systems, client–server, cross-machine microservices) are four binding forms of one structure:

- **Contractual slot**: the interaction surface exists at compile time as "type = contract" (the external system's signature: message type, direction, obligation class, modality); the implementation is installed in the future / at runtime, and the external system is **absent from the compilation graph**—each constitution compiles independently without compiling the counterpart. The parametrization is **binding time × binding mechanism**: device trees (build-time compilation + runtime enumeration, match-table pairing), plugin systems (runtime loading, interface registration), client–server (connection establishment, address + protocol handshake), microservices (network carrier, contract + delivery states).
- **Decidability boundary**: every conformance "about an existent object" is decidable (proof-like: contract conformance ①③, self-implementation internal properties ①②③); every behavior "about a future implementation or external semantics" is undecidable at compile time—only sampling (runtime tests) or honest declaration (④) plus monitoring (empirical-D) apply.
- **Placeholder role**: a placeholder makes host-internal integration closed at compile time—it verifies **contract–host conformance**, not **counterpart–host conformance** (placeholder passing ⊬ counterpart passing); counterpart behavioral equivalence is a runtime matter (T6 class).
- **Decidability panorama**:

| Verification target | Decidability | When |
|---|---|---|
| Contract conformance (slot ↔ contract) | decidable (②③) | compile/assembly |
| Self-implementation internal properties (composition/totality/obligations) | decidable (①②③) | compile/assembly |
| Placeholder/counterpart conformance (when implemented) | decidable | build time |
| Cross-end behavioral equivalence | sampling | runtime |
| Future implementation behavior | undecidable → ④ + monitoring | future/runtime |
| Interaction-sequence semantics (timing/teardown/latency) | empirical-D (bounds + monitoring) | runtime |

- **Theoretical placement**: the largest-scale application of concept 4 (typed hole); the inter-system seam proposition (multiple constitutions interconnected via inter-system carriers, contract conformance ∀ verified at compile time, existence ∃ bound at runtime, absence = delivery state Closed); also the cross-project form of Props. 7.1/7.3. Industrial convergences: the Linux device model (driver–device matching), FIDL/protocol schemas (protobuf/OpenAPI), consumer-driven contract testing (Pact), FFI/ABI boundaries—all specializations of one structure; axiom provides the minimal kernel (typed hole + four modalities + obligation algebra) and, via the law of stratification (Thm. 9.6), locates the undecidable part at the seam (L₂) instead of hiding it.

---

## 8. 构成最优性 / Constitutional Optimality

**引理 8.0（收割侧 / The Harvest Side）**。D 放置的正当性由下游解锁的可判定性追溯支付：假设全函数 ⟹ 买回编译期全函数检查；假设端口同构（C1 裁定）⟹ 买回无转换的自组合 `Rep`；假设 Kahn 缓冲 ⟹ 买回环的类型安全。放置最优性必须同时计：前向极小（D 尽量小）与反向多产（解锁尽量多）。

**Lemma 8.0 (Harvest Side)**. The justification of a D-placement is paid retroactively by the decidability it unlocks downstream: assuming totality buys compile-time totality checks; assuming port-symmetry (the C1 ruling) buys conversion-free self-composition (`Rep`); assuming Kahn buffering buys type-safe cycles. Placement optimality must count both forward minimality (small D) and backward productivity (much unlocked).

**猜想 8.1（构成最优性 / Constitutional Optimality）**。在任务相关义务集固定的前提下，构成间存在帕累托偏序：C₁ ≼ C₂ 当且仅当 C₁ 的义务模态强度剖面逐点不弱于 C₂，且未展出假定不多于 C₂。三点限定：(i) 剖面而非规模——|P| 可通过削弱谓词作弊（指标腐化），强度剖面不可；(ii) 未展出而非静默——④声明的残余已被展出，不计入；(iii) 收割侧（引理 8.0）进入比较——同等剖面下解锁多者优先。若成立，公理放置成为可优化对象。

**Conjecture 8.1 (Constitutional Optimality)**. With the task-relevant obligation set fixed, a Pareto order exists on constitutions: C₁ ≼ C₂ iff C₁'s obligation-modality strength profile is pointwise no weaker than C₂'s and C₁'s unexhibited assumptions are no more numerous. Three qualifications: (i) profile, not size—|P| is cheatable by weakening predicates (metric gaming), a strength profile is not; (ii) unexhibited, not silent—a ④-declared residual has been exhibited and does not count; (iii) the harvest side (Lemma 8.0) enters comparison—at equal profiles, more unlocking wins. If it holds, axiom placement becomes optimizable.

**开放问题 8.2（放置唯一性 / Uniqueness of Placement）**。给定（文法 G，软件形式 F），诚实放置 C(F) 是否唯一？倾向：不唯一——理由已精确化：剖面 × 收割的二维偏序一般有多点前沿，不唯一是常态而非反例。内核与服务对同一义务类的放置不同即其例证。稳定性补充：在生产压力下持续的放置形态可类比演化稳定策略。锚点：Maynard Smith & Price 1973（ESS）。

**Open Problem 8.2 (Uniqueness of Placement)**. Given (grammar G, form F), is the honest placement C(F) unique? Inclination: no—and the reason is now precise: a two-dimensional order (profile × harvest) generally has a multi-point frontier; non-uniqueness is the normal case, not a counterexample. Kernels and services placing the same obligation class differently is the running instance. Stability addendum: placement shapes persisting under production pressure are analogous to evolutionarily stable strategies. Anchor: Maynard Smith & Price 1973 (ESS).

**开放问题 8.3（第五态 / The Fifth State）**。模态 ①②③④ 之外是否存在第五态——未展出的隐含？主张：否；未展出的隐含不是模态，是构成违反（定义 1.5 的违例）。此主张使模态体系完备为一个格：{①②③④} ∪ {∅}——∅ 为违例类（零点）；每条义务恰占一格，否则整个构成失效。诚实规则由此从道德条款提升为构成的定义条件。

**Open Problem 8.3 (The Fifth State)**. Beyond modalities ①②③④, does a fifth state—unexhibited implicitness—exist? Claim: no; unexhibited implicitness is not a modality but a constitution violation (a violation of Definition 1.5). The claim completes the modality system as a lattice: {①②③④} ∪ {∅}, where ∅ is the violation class (zero point); every obligation occupies exactly one cell or the constitution fails outright. This raises the honesty rule from a moral clause to a defining condition of constitution.

**命题 8.4（迁移诚实 / Transition Honesty）**。放置有版本史：构成会演化（改名、降级、物理层获得义务）。诚实的单位不只是快照，还有迁移——弃用横幅、对账记录、变更日志是 M 的时间实践物；每次审计都是在漂移之后重建不动点。锚点：Lakatos 1963–64（纲领的进步/退化沿时间判定）。

**Proposition 8.4 (Transition Honesty)**. Placements have histories: constitutions evolve (renames, downgrades, physics acquiring obligations). The unit of honesty is not only the snapshot but the transition—deprecation banners, reconciliation records, and changelogs are M's temporal practice artifacts; each audit re-establishes a fixpoint after drift. Anchor: Lakatos 1963–64.

**命题 8.5（承认规则 / Rule of Recognition）**。"三分区由谁决定"的答案是：由一个被承认的程序决定——宪法修正程序。Grundnorm 要成为规范而非个人趣味，预设一个批准共同体。仓库封闭清单"新增概念须经集体裁定显式作出、不容隐性新规则"即该程序的现行实例。锚点：Hart 1961（承认规则：官员据以识别有效法律的社会规则）；Lewis 1969（惯例＝解决协调问题的均衡）；Kelsen 1934（基本规范预设法律秩序）；Wittgenstein 1969（《论确定性》：根基的确定性立于行动之中，不立于论证链条）。

**Proposition 8.5 (Rule of Recognition)**. Who decides the tri-partition? A recognized procedure—the constitutional amendment process. For the Grundnorm to be normative rather than personal taste presupposes a ratifying community. The repo closure checklist ("new concepts require explicit collective adjudication; no hidden new rules") is the current instance of this procedure. Anchors: Hart 1961 (the rule of recognition: the social rule by which officials identify valid law); Lewis 1969 (convention as an equilibrium of a coordination problem); Kelsen 1934 (the basic norm presupposes a legal order); Wittgenstein 1969 (*On Certainty*: the certainty of the root stands in action, not in an argument chain).

---


**开放问题 8.4（初对象猜想 / Initial-Object Conjecture）**：猜想——五概念呈现
（生成元＋关系）在"安全构造学科"的范畴中是**初对象候选**：任何其他可审计构造体系
存在唯一保持判定时刻的射入本呈现。未证；仅与开放问题 8.2（放置唯一性）并列登记，
作为元问题树的收敛点假说。

## 9. 对 axiom 与 axiom-semantics 的应用 / Application to axiom and axiom-semantics

- **构成**：五概念（端口体 / T1 对偶 / 组合封闭 / 代换绑定 / 激活）是词汇表；其中良构部分经类型系统执行为文法区 G——`Conforms`/`Wire` 的 T1 配对即其类型论执行（命题 4.3）。谱系诚实：五概念是从既有代码溯回概括、后经批准的宪法条款（foundations §8.1 封闭清单），其发生史属于展出内容，不呈现为赤裸公设。公理区成员须按定义 1.4 分类：全函数、纯度、Moore 声明 ∈ 逻辑-D；**零成本相对等式 ∈ 经验-D**——它是可证伪的经验命题，已有带噪声底的实测证据（Δ 在噪声区间内），随时可被新工具链推翻，不是公理。消除性验证（C1 精确同型等）∈ P 的机械。
- **认识论强度谱**：模态 ② 见证 / ③ 验证 / ④ 声明 = 定理 3.1 的落地形式；诚实纪律（contract.rs"声明看起来已验证比诚实缺口更糟"）= 推论 3.2 的执行。
- **语义 = ④ 声明，与类型判定互补**：语义真值不在任何分层阶 tₖ 被系统验证（semantics-constitution 定义卷 D6）；类型判定与语义互补、不重叠——可判定者归 ①②③，不可系统验证者只允许 ④。axiom-semantics 的语义函数 ⟦·⟧ 是 σ 的范畴级推广（semantics-constitution 前置"层位身份"段）。
- **semantics = 义务代数的机械**：物理层自分层（boundary-ontology §9 在物理层内的递归应用）；义务类语法（投递态 × 资源类 × 引用有效 × 生命周期）为物理层的前四阶公理；`assemble_link`/`assemble_seam` 为装配校验（模态③ 机械）。
- **审计 = 展出，非证明**：每一次审计、每一处标注，是 M 在行动中的回答（推论 2.2 与 3.2 的实践对应）。
- **最深的公理**：不是零成本，不是全函数，而是**显式构成的意志**（选择在显式构成下建造，而非在隐含假定下漂移）；它是 Grundnorm——不可证明，只能行动。

**Application.** The five concepts are the vocabulary; their well-formed part is executed as the grammar region G by the type system—the `Conforms`/`Wire` T1 pairing is its type-theoretic execution (Proposition 4.3). Genealogical honesty: the five concepts are an abductive generalization from existing code, later ratified as a constitutional clause (the foundations §8.1 closure checklist); their genesis belongs to the exhibition, not to bare postulation. Axiom-region members must be classified per Definition 1.4: totality, purity, and the Moore declaration ∈ logical-D; **the zero-cost relative equality ∈ empirical-D**—a falsifiable empirical claim with measured evidence under a noise floor (Δ within noise bands), refutable by any new toolchain, not an axiom. Elimination verifications (C1 exact sameness, etc.) are machinery of P. Modalities ①②③④ are Theorem 3.1 realized; the honesty discipline is Corollary 3.2 executed. The runtime is the machinery of the obligation algebra; the obligation-class grammar is the first four instances of physical self-stratification. Audit is exhibition, not proof. The deepest axiom is not zero cost nor totality but the will to explicit constitution—a Grundnorm held by a ratifying community (Proposition 8.5): unprovable, actable only.

---

## 10. 结论 / Conclusion

**中文**：本文把元问题 M 形式化为构成 C = (G, P, D) 的三分区问题，给出其代数核心（D 是 P 相对推理规则的生成元集，M ＝ 基选取）及八个子命题：回归三难（定理 2.1）、证明必要性分层（定理 3.1，放置相对）、诚实超形式（推论 3.2）、合法性构成相对性（命题 4.1–4.3）、外审命题（命题 5.1）、义务源于构成的 Noether 形态（命题 6.1），以及形式即放置与混合形式、实现域标准化六元组（命题 7.1/7.3/7.4）、构成最优性（猜想 8.1，帕累托形态＋收割侧）、迁移诚实与承认规则（命题 8.4/8.5）。结论：(1) 基底必被安置而非被证明，安置只可展出；(2) 证明必要性是分层的且相对于放置，其强度沿谱系递增；(3) 最深的公理是显式构成的意志，它由批准共同体经修正程序持有。M 不被解决，M 被实践——实践即在不漂移的意义上维护一个极小、被完整展出的基。

**English**: This paper formalizes the meta-question M as the tri-partition problem of a constitution C = (G, P, D), gives its algebraic core (D as a generator set of P relative to the inference rules; M as basis selection), and states eight sub-propositions: the Regress Trilemma (Theorem 2.1), placement-relative Stratified Necessity of Proof (Theorem 3.1), Honesty as Extra-Formal (Corollary 3.2), Constitution-Relativity of Legality (Propositions 4.1–4.3), External Audit (Proposition 5.1), Obligations Derive from Constitution—the Noether Form (Proposition 6.1)—plus Forms as Placements, Hybrid Forms, and the six-tuple Standardization of Implementations (Propositions 7.1/7.3/7.4), Constitutional Optimality (Conjecture 8.1, Pareto form with Harvest Side), and Transition Honesty with the Rule of Recognition (Propositions 8.4/8.5). Conclusions: (1) the base must be placed, not proved, and placement can only be exhibited; (2) the necessity of proof is stratified and placement-relative, its strength ascending a spectrum; (3) the deepest axiom is the will to explicit constitution, held by a ratifying community through an amendment procedure. M is not solved; M is practiced—and practice means maintaining a minimal, fully exhibited base against drift.

---

## 参考文献 / References

### A. 逻辑与递归论 / Logic and Recursion Theory

[1] K. Gödel. Über formal unentscheidbare Sätze der Principia Mathematica und verwandter Systeme I. *Monatshefte für Mathematik und Physik*, 38:173–198, 1931.

[2] A. Tarski. Der Wahrheitsbegriff in den formalisierten Sprachen. *Studia Philosophica*, 1:261–405, 1936.

[3] H. G. Rice. Classes of recursively enumerable sets and their decision problems. *Transactions of the American Mathematical Society*, 74(2):358–366, 1953.

[4] S. C. Kleene. Recursive predicates and quantifiers. *Transactions of the American Mathematical Society*, 53(1):41–73, 1943.

[5] G. Gentzen. Die Widerspruchsfreiheit der reinen Zahlentheorie. *Mathematische Annalen*, 112:493–565, 1936.

[6] S. G. Simpson. *Subsystems of Second Order Arithmetic*. Springer, 1999.（逆向数学；另见 H. Friedman 1970s 系列）

[7] R. Milner. A Theory of Type Polymorphism in Programming. *Journal of Computer and System Sciences*, 17(3):348–375, 1978.

### B. 数学基础与哲学 / Foundations of Mathematics and Philosophy

[8] F. W. Lawvere. An Elementary Theory of the Category of Sets. *Proceedings of the National Academy of Sciences*, 52:1506–1511, 1964.（ETCS）

[9] W. A. Howard. The Formulae-as-Types Notion of Construction. 1969, in *To H. B. Curry: Essays on Combinatory Logic* (1980).（Curry-Howard 同构）

[10] H. Albert. *Traktat über kritische Vernunft*. 1968.（Münchhausen 三难）

[11] W. Sellars. Empiricism and the Philosophy of Mind. *Minnesota Studies in the Philosophy of Science*, 1:253–329, 1956.

[12] K. Popper. *Logik der Forschung*. 1934.

[13] I. Lakatos. Proofs and Refutations. *British Journal for the Philosophy of Science*, 14:1–25, 1963–64.

[14] L. Wittgenstein. *Philosophische Untersuchungen*. 1953.（规则遵循考量：文法区的实践面）

[14b] L. Wittgenstein. *On Certainty*（Über Gewissheit）. 1969.（命题 8.5：根基的确定性立于行动）

[15] Sextus Empiricus. *Outlines of Pyrrhonism*（约公元 180 年；Agrippa 五式/无判据三难的现存来源）。

### C. 法学、博弈论与生物学 / Law, Game Theory, and Biology

[16] H. Kelsen. *Reine Rechtslehre*. 1934.（Grundnorm）

[17] H. L. A. Hart. *The Concept of Law*. Clarendon Press, 1961.（承认规则）

[18] D. Lewis. *Convention: A Philosophical Study*. Harvard University Press, 1969.

[19] J. Maynard Smith, G. R. Price. The Logic of Animal Conflict. *Nature*, 246:15–18, 1973.（ESS）

### D. 物理学 / Physics

[20] E. Noether. Invariante Variationsprobleme. *Nachrichten von der Gesellschaft der Wissenschaften zu Göttingen*, 235–257, 1918.

[21] C. N. Yang, R. L. Mills. Conservation of Isotopic Spin and Isotopic Gauge Invariance. *Physical Review*, 96(1):191–195, 1954.（规范原理）

### E. 工程与系统 / Engineering and Systems

[22] M. J. C. Gordon, A. J. R. G. Milner, C. P. Wadsworth. *Edinburgh LCF*. LNCS 78, Springer, 1979.

[23] N. G. de Bruijn. The Mathematical Language AUTOMATH（及 de Bruijn 判据传统，1970/1994）。

[24] G. Klein et al. seL4: Formal Verification of an OS Kernel. *SOSP 2009*, 207–220.

[25] J. H. Saltzer, M. D. Schroeder. The Protection of Information in Computer Systems. *Proceedings of the IEEE*, 63(9):1278–1308, 1975.

[26] C. A. R. Hoare. The Emperor's Old Clothes. *Communications of the ACM*, 24(2):75–83, 1981.

[27] L. Lamport. *Specifying Systems: The TLA+ Language and Tools for Hardware and Software Engineers*. Addison-Wesley, 2002.

### 内部文档 / Internal Documents

[28] [`boundary-ontology.md`](boundary-ontology.md) §9 分层律；§2 四轴；§5 三归宿；§6 双层信任架构。

[29] [`frontier-notes.md`](frontier-notes.md)（可判定性阶梯、产物分析粒度阶梯等前沿注记）。