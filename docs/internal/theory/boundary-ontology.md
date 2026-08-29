# 边界本体论：代数合法性与机器可达性的四轴结构，及双层信任架构

# Boundary Ontology: Four-Axis Structure of Algebraic Legality and Machine Reachability, and Two-Layer Trust Architectures

> **性质**：I1 层理论注记（`docs/internal/theory/`，不入 git）。**非已实现、非承诺**。
> 术语以中英双语并列定义，二语构成同一概念的唯一指称，不另设语际映射。
> 记号：W 配置空间；σ 语义函数；ρ 重接线；Obs 可观察行为集；A 代数闭包；M 机器闭包；T 类型可表达闭包；K 内核不变量集；J 准入接缝。

## 摘要 / Abstract

**中文摘要**：本文研究软件结构中"可定义"与"可执行"之间的关系。主要内容：(1) 建立四个相互独立的过滤轴——可定义性、可实现性、可行性、可验证性（第 2 节）；(2) 证明代数闭包与机器闭包互不包含（第 3 节）；(3) 给出合法重接线的判定准则 σ∘ρ = σ，证明合法性是 (载体, 语义) 的二元属性而非操作的文本属性，并以设备树子节点排列为例证（第 4 节）；(4) 证明类型系统不可完备表达的代数合法对象，其落实方式可穷尽为三归宿：接缝声明、降级编码、类型强化（第 5 节）；(5) 将 Rust 安全/非安全边界与 axiom 抽象/物理边界统一为双层信任架构（LCF 型），证明二者准入集合不同且极性相反（第 6 节）。

**English Abstract**: This paper studies the relation between definability and executability in software structures. Contents: (1) four mutually independent filtering axes are established—definability, computability, feasibility, verifiability (Section 2); (2) the algebraic closure and the machine closure are shown neither to contain the other (Section 3); (3) the criterion σ∘ρ = σ for lawful rewiring is given, and legality is proved to be a binary attribute of the pair (carrier, semantics) rather than of the operation text, evidenced by device-tree child permutation (Section 4); (4) the realization of algebraically lawful objects not fully expressible in a type system is proved exhaustive over three destinations: seam admission, encoding degradation, type strengthening (Section 5); (5) the Rust safe/unsafe boundary and the axiom abstract/physical boundary are unified as two-layer trust architectures of the LCF type, with differing admission sets and opposite polarity (Section 6).

**关键词 / Keywords**：闭包系统；配置空间；语义保持；商类型；接缝；双层信任架构；四轴过滤 · closure system; configuration space; semantics preservation; quotient type; seam; two-layer trust architecture; four-axis filtering

---

## 0. 引言 / Introduction

**中文**：软件结构的合法性判断隐含于某一闭包系统之内；同一对象在不同闭包系统中具有不同的合法性。本文将"合法"分解为四个相互独立的过滤轴，并证明不存在归约链（命题 2.5）。第 3 节证明：代数闭包与机器闭包各含对方不可达的对象，故"代码是代数的子集"不成立，反之亦然。第 4 节研究动态重接线——一类在运行期改变配置之间关系的操作——给出合法性的语义保持准则（定义 4.1）；该准则使"重接线是否被允许"成为 (载体, 语义) 的二元属性。第 5 节处理"代数合法、类型不可完备表达"的对象，证明三归宿的穷尽性与互斥性（定理 5.1）。第 6 节将 Rust 与 axiom 的边界构造统一为同一架构模式，并指出其极性差异。第 7 节将前述结构映射回 axiom 的四模态纪律；第 8 节为结论。

**English**: Lawfulness of a software structure is judged within some closure system; the same object has different lawfulness in different closure systems. This paper decomposes "lawfulness" into four mutually independent filtering axes and proves that no reduction chain exists among them (Proposition 2.5). Section 3 proves that the algebraic closure and the machine closure each contain objects unreachable by the other; hence "code is a subset of algebra" holds in neither direction. Section 4 studies dynamic rewiring—operations that alter relations between configurations at run time—and gives the semantics-preserving criterion of lawfulness (Definition 4.1); by this criterion, whether rewiring is permitted is a binary attribute of the pair (carrier, semantics). Section 5 treats objects that are algebraically lawful yet not fully expressible in a type system, and proves the exhaustiveness and mutual exclusiveness of three destinations (Theorem 5.1). Section 6 unifies the Rust and axiom boundary constructions as one architectural pattern and states their polarity difference. Section 7 maps the results back to the axiom four-modality discipline; Section 8 concludes.

---

## 1. 记号与基本定义 / Notation and Basic Definitions

**定义 1.1（闭包系统 / Closure System）**。设 U 为全集，Φ 为算子集，G ⊆ U 为生成元集。以 cl(G, Φ) 记包含 G 且对 Φ 中每个算子封闭的最小集合；cl(G, Φ) 称为 (G, Φ) 的闭包。

**Definition 1.1 (Closure System)**. Let U be a universe, Φ a set of operators, and G ⊆ U a set of generators. Let cl(G, Φ) denote the least set containing G and closed under every operator in Φ; cl(G, Φ) is the closure of (G, Φ).

**定义 1.2（配置空间 / Configuration Space）**。配置空间 W 是系统全部配置的集合。语义函数 σ: W → Obs 将每个配置 x ∈ W 映射到其可观察行为 σ(x)。

**Definition 1.2 (Configuration Space)**. The configuration space W is the set of all configurations of a system. The semantic function σ: W → Obs maps each configuration x ∈ W to its observable behavior σ(x).

**定义 1.3（重接线 / Rewiring）**。重接线是部分映射 ρ: W ⇀ W，即定义域 dom(ρ) ⊆ W 的变换；ρ 表示在运行期将配置 x 改为配置 ρ(x) 的操作。

**Definition 1.3 (Rewiring)**. A rewiring is a partial mapping ρ: W ⇀ W, i.e., a transformation with dom(ρ) ⊆ W; ρ denotes an operation that changes configuration x to ρ(x) at run time.

**定义 1.4（商类型 / Quotient Type）**。设 ≃ ⊆ W × W 为等价关系。商空间 W/≃ 以等价类为元素。若类型系统可以类型的形态表示 W/≃ 及其上的运算，则称该商类型可完备表达；否则称其不可完备表达。

**Definition 1.4 (Quotient Type)**. Let ≃ ⊆ W × W be an equivalence relation. The quotient space W/≃ is the set of equivalence classes. If a type system can represent W/≃ and its operations in type form, the quotient type is fully expressible; otherwise it is not fully expressible.

**定义 1.5（接缝 / Seam）**。接缝是系统边界上的准入通道 J：经 J 允许执行不满足内核不变量集 I_K 的操作，且此类操作不在内核中验证。

**Definition 1.5 (Seam)**. A seam is an admission channel J on the boundary of a system: operations that do not satisfy the kernel invariant set I_K are admitted through J and are not verified within the kernel.

**定义 1.6（声明与证明 / Declaration and Proof）**。声明是系统接受为真的命题 h，其真值不由系统验证；证明是系统验证为真的命题。二者在验证责任上互补，不可互相替代。

**Definition 1.6 (Declaration and Proof)**. A declaration is a proposition h accepted as true by the system, whose truth is not verified by the system; a proof is a proposition verified by the system. The two are complementary in verification responsibility and non-interchangeable.

**定义 1.7（载体 / Carrier）**。载体是物理层的执行实体，承担运行期激活与效应承载；载体须以抽象层接口（PortCell/Conforms）的形态呈现方可被系统接受（§8.4，运行时为物理层实现用例）。

**Definition 1.7 (Carrier)**. A carrier is an execution entity of the physical layer, responsible for run-time activation and effect bearing; a carrier is accepted by the system only if it presents the interface shape of the abstract layer (PortCell/Conforms) (§8.4; the runtime is the physical-layer use-case of the implementation).

---

## 2. 四轴过滤 / Four-Axis Filtering

**定义 2.1（可定义性 / Definability）**。对象 x 可定义（关于闭包系统 (G, Φ)），当且仅当 x ∈ cl(G, Φ)。

**Definition 2.1 (Definability)**. An object x is definable (with respect to a closure system (G, Φ)) iff x ∈ cl(G, Φ).

**定义 2.2（可实现性 / Computability）**。对象 x 可实现，当且仅当存在图灵机枚举或判定 x。以 M 记全部递归可枚举结构的机器闭包。

**Definition 2.2 (Computability)**. An object x is computable iff some Turing machine enumerates or decides x. Let M denote the machine closure of all recursively enumerable structures.

**定义 2.3（可行性 / Feasibility）**。对象 x 可行，当且仅当存在实现 x 的程序 p，且 p 在给定资源上界（时间、空间）内终止。

**Definition 2.3 (Feasibility)**. An object x is feasible iff there exists a program p implementing x such that p terminates within a given resource bound (time, space).

**定义 2.4（可验证性 / Verifiability）**。对象 x 的关涉性质 I(x) 可验证，当且仅当 I 的判定问题可判定，即存在决策程序判定 I(x) 真值。

**Definition 2.4 (Verifiability)**. A property I(x) of an object x is verifiable iff the decision problem of I is decidable, i.e., some decision procedure determines the truth of I(x).

**命题 2.5（四轴独立 / Independence of the Four Axes）**。对任意三个轴的合取无法蕴含第四个轴的成立；四轴之间不存在归约链。

**Proposition 2.5 (Independence of the Four Axes)**. No conjunction of three axes entails the fourth; no reduction chain exists among the four axes.

**证明**：以四个构造性实例确立独立性。

(1) 可定义 ∧ ¬可实现：设 Ω = {⟨M⟩ : M 停机}，Ω̄ 为 Ω 在全集下的补。由对角线论证，Ω̄ 非递归可枚举，故 Ω̄ ∉ M。取闭包系统 (G = {Ω}, Φ = {补运算})，则 Ω̄ ∈ cl(G, Φ)（可定义），且 Ω̄ ∉ M。

(2) 可实现 ∧ ¬可定义（对正则代数）：括号平衡语言 Bal 属于上下文无关语言而非正则语言，故 Bal ∉ cl(正则生成元, Kleene 算子)；Bal ∈ M（下推自动机可判定）。

(3) 可实现 ∧ ¬可行：判定 SAT 可满足性是可判定的（有限搜索终止于有限赋值空间），故对象 ∈ M；在 P ≠ NP 假设下，不存在多项式资源上界的算法，故对多项式界不可行。

(4) 可实现 ∧ ¬可验证：取 p 为不自停程序；p ∈ M（构造即实现）。性质"p 是否停机"为停机问题的实例，不可判定。

由 (1)–(4)，每一轴均可单独与其余轴的任意组合并存，故无归约链。∎

**注记 2.6**。四轴是独立过滤器而非层级链："代数合法但工程不合法"（(3) 类）、"代数合法但机器不合法"（(1) 类）、"机器合法但代数不合法"（(2) 类）分别位于不同轴，不可互相化归。

**Remark 2.6.** The four axes are independent filters, not a hierarchy: "algebraically lawful yet engineering-illegal" (type (3)), "algebraically lawful yet machine-illegal" (type (1)), and "machine-lawful yet algebraically illegal" (type (2)) lie on different axes and are mutually irreducible.

**命题 2.7（第五轴：目的相容 / The Fifth Axis: Admissibility）**。除四轴（可定义、可实现、可行、可验证；命题 2.5）外，存在独立的第五轴——**目的相容（admissibility）**：一个对象可采纳，当且仅当它不与构造的目的条款相抵触，即它不处于**退化态（degenerate state）**。退化态 = 通过四轴（可写、可跑、预算内、可判）却违背目的条款的语义自我否定态；目的过滤器可部分机械化：目的一旦成文，退化的可判子集经模态 ①② 拒绝（如 CAP≥1 门槛、Result 车道（lane）、typestate），残余为价值论声明（模态 ④）。

**证明（纲要）**。四轴合取不蕴含可采纳：构造容量为零的背压队列满足四轴（可写、可跑、预算内、判定平凡），却违背"背压即不丢消息"的目的条款，故不被采纳；因此第五轴独立于四轴。机械化方向：目的条款成文 ⟹ 退化判定转化为可判谓词（编译期门/模态 ①②）或诚实声明（模态 ④）。∎

**Remark 2.7 (The Fifth Axis: Admissibility).** Beyond the four axes (definability, realizability, feasibility, verifiability; Prop. 2.5) there is an independent fifth axis — **admissibility** (purpose-compatibility): an object is admissible iff it does not contradict the purpose clause of its construction, i.e. it is not in a **degenerate state**. A degenerate state = one that passes the four axes (writable, runnable, within budget, decidable) yet semantically negates the purpose clause; the purpose filter is partially mechanizable: once the purpose is written down, the decidable subset of degeneracy is rejected via modalities ①② (e.g. the CAP≥1 gate, the Result lane, typestates), and the residue is an honest declaration (modality ④).

**Proof (sketch).** The conjunction of the four axes does not imply admissibility: a zero-capacity backpressure queue satisfies all four axes (writable, runnable, within budget, trivially decidable) yet violates the purpose clause "backpressure means no message loss", hence is not adopted; therefore the fifth axis is independent of the four. Mechanization direction: purpose written down ⟹ degeneracy testing reduces to decidable predicates (compile-time gates / modalities ①②) or to honest declarations (modality ④). ∎

**注记 2.8（选择轴包含链 / Selection-Axis Inclusion）**。在选择轴（对象经四轴过滤后实际被产生的集合）上成立严格包含链：工程实现 ⊆ 代码可实现 ⊆ 逻辑可定义；收缩因子分别是机器约束与目的过滤器。在表达轴（闭包之间的可表达性关系，定理 3.1 的对象）上不成立（定理 3.1：代数与机器闭包互不包含）。目的过滤器与四轴的关系：四轴回答"能否构造"，第五轴回答"是否应当构造"。

**Remark 2.8 (Selection-Axis Inclusion).** On the selection axis (the set of objects actually produced after the four-axis filtering) the strict inclusion chain holds: engineering implementations ⊆ machine-realizable ⊆ logically definable; the contraction factors are machine constraints and the purpose filter respectively. It does not hold on the expression axis (Thm. 3.1: the algebraic and machine closures do not contain each other). Relation between the purpose filter and the four axes: the four axes answer "can it be constructed", the fifth answers "should it be constructed".

---

## 3. 闭包交集定理 / The Closure Intersection Theorem

**定理 3.1（闭包交集 / Closure Intersection）**。存在代数闭包 A 与机器闭包 M，使得 A ∖ M ≠ ∅ 且 M ∖ A ≠ ∅。即：两个闭包各含对方不可达的对象，且交集 A ∩ M 是真包含于双方闭包。

**Theorem 3.1 (Closure Intersection)**. There exist an algebraic closure A and a machine closure M such that A ∖ M ≠ ∅ and M ∖ A ≠ ∅. That is, each closure contains objects unreachable by the other, and A ∩ M is a proper subset of both closures.

**证明**。(i) A ∖ M 方向：取 A₀ = cl({Ω}, {补运算})（记号同命题 2.5(1)），则 Ω̄ ∈ A₀ 且 Ω̄ ∉ M，故 A₀ ∖ M ≠ ∅。(ii) M ∖ A 方向：取 A₁ = 正则代数闭包（Kleene 闭包），Bal ∈ M ∖ A₁，故 M ∖ A₁ ≠ ∅。由 (i)(ii)，序偶 (A₀, A₁; M) 使两个方向各自非空。∎

**Proof.** (i) Direction A ∖ M: let A₀ = cl({Ω}, {complement}) (notation of Proposition 2.5(1)); then Ω̄ ∈ A₀ and Ω̄ ∉ M, hence A₀ ∖ M ≠ ∅. (ii) Direction M ∖ A: let A₁ be the regular-algebra closure (Kleene closure); Bal ∈ M ∖ A₁, hence M ∖ A₁ ≠ ∅. By (i)(ii), the pair (A₀, A₁; M) yields both directions nonempty. ∎

**推论 3.2**。"代码能力是代数逻辑的子集"在两个方向上均不成立：对任意"类型可表达闭包 T ⊆ A 且 T ⊆ M"的表述，T ∩ M ⊊ M 与 T 之内的代数成员缺失可同时成立；类型层与机器层各有对方不可达对象。

**Corollary 3.2.** "Code capability is a subset of algebraic logic" holds in neither direction: for any expressible typing closure T with T ⊆ A and T ⊆ M, both T ∩ M ⊊ M and the absence of algebraic members inside T may hold simultaneously; the type layer and the machine layer each contain objects unreachable by the other.

---

## 4. 合法重接线 / Lawful Rewiring

**定义 4.1（合法 / Lawful）**。部分映射 ρ: W ⇀ W 合法，当且仅当对任意 x ∈ dom(ρ)，σ(ρ(x)) = σ(x)。以复合记法写作 σ∘ρ = σ（在 dom(ρ) 上）。

**Definition 4.1 (Lawful)**. A partial mapping ρ: W ⇀ W is lawful iff σ(ρ(x)) = σ(x) for every x ∈ dom(ρ); in composition notation, σ∘ρ = σ (on dom(ρ)).

**命题 4.2（群胚结构 / Groupoid Structure）**。全体合法且可逆的重接线在复合下构成群胚 G(W, σ)：对象为配置，箭头为合法映射；可逆合法映射之全体构成各纤维 σ⁻¹(b)（b ∈ Obs）上对称群的弱积。

**Proposition 4.2 (Groupoid Structure)**. All lawful and invertible rewirings form, under composition, a groupoid G(W, σ): objects are configurations, arrows are lawful mappings; the lawful invertible mappings constitute the weak product, over fibers σ⁻¹(b) (b ∈ Obs), of symmetric groups.

**证明**。对每个 b ∈ Obs，纤维 σ⁻¹(b) 上任意置换保持 σ，故合法；全体置换构成对称群 Sym(σ⁻¹(b))。合法映射可逆当且仅当其逐纤维为双射。可逆合法映射在复合下封闭（σ∘(ρ₂∘ρ₁) = (σ∘ρ₂)∘ρ₁ = σ），且每箭头有逆，满足群胚公理。∎

**Proof.** For each b ∈ Obs, every permutation of the fiber σ⁻¹(b) preserves σ, hence is lawful; all such permutations form the symmetric group Sym(σ⁻¹(b)). A lawful mapping is invertible iff it is a bijection on each fiber. Invertible lawful mappings are closed under composition (σ∘(ρ₂∘ρ₁) = (σ∘ρ₂)∘ρ₁ = σ), and every arrow has an inverse; the groupoid axioms hold. ∎

**命题 4.3（二元属性 / Binary Attribute）**。重接线的合法性是序对 (W, σ) 的属性，而非操作文本 ρ 的属性。具体地：存在 (W, σ₁, ρ) 与 (W, σ₂, ρ)，使 ρ 在 (W, σ₁) 下合法、在 (W, σ₂) 下不合法。

**Proposition 4.3 (Binary Attribute)**. Lawfulness of a rewiring is an attribute of the pair (W, σ), not of the operation text ρ. Specifically, there exist (W, σ₁, ρ) and (W, σ₂, ρ) such that ρ is lawful under (W, σ₁) and unlawful under (W, σ₂).

**证明**。取 W = {x₁, x₂}，ρ 为互换 x₁ ↔ x₂ 的置换。设 σ₁(x₁) = σ₁(x₂) = a（无序/集合语义），则 σ₁∘ρ = σ₁，合法。设 σ₂(x₁) ≠ σ₂(x₂)（有序/序列语义），则 σ₂(ρ(x₁)) = σ₂(x₂) ≠ σ₂(x₁)，不合法。操作文本 ρ 相同，合法性相反。∎

**Proof.** Let W = {x₁, x₂} and ρ be the transposition x₁ ↔ x₂. Let σ₁(x₁) = σ₁(x₂) = a (unordered/set semantics): then σ₁∘ρ = σ₁, lawful. Let σ₂(x₁) ≠ σ₂(x₂) (ordered/sequence semantics): then σ₂(ρ(x₁)) = σ₂(x₂) ≠ σ₂(x₁), unlawful. The operation text ρ is identical; lawfulness is opposite. ∎

**例 4.4（设备树 / Device Tree）**。父节点的子节点列表在模式中的语义键为 (name, unit-address) 与 reg 属性，而非列表位次；即语义函数 σ₁ 将 List(Node) 商为按键的集合。任意排列 π ∈ Sₙ 满足 σ₁∘π = σ₁，合法。反之，若运行策略以列表位次为探测次序且次序影响驱动绑定，则 σ₂ 使同一 π 不合法。结论：排列"缺少意义"这一事实，恰为商结构存在的证据——意义的缺失即载体已是商。

**Example 4.4 (Device Tree)**. The semantic key of a parent's child list in the schema is the pair (name, unit-address) and the reg property, not the list position; i.e., the semantic function σ₁ quotients List(Node) into a set keyed by entry. Every permutation π ∈ Sₙ satisfies σ₁∘π = σ₁, hence lawful. Conversely, if the run-time strategy uses list position as probe order and order affects driver binding, then σ₂ renders the same π unlawful. Conclusion: the fact that the permutation "lacks meaning" is evidence that the carrier is already a quotient—the absence of meaning is the quotient.

**推论 4.5（受约束的动态 / Bounded Dynamism）**。动态重接线可受约束，当且仅当许可配置集可表示为商类型（定义 1.4）。若不可表示，合法性的判定退化为运行期校验（对应 axiom 模态 ③）或声明（对应模态 ④）。

**Corollary 4.5 (Bounded Dynamism)**. Dynamic rewiring can be constrained iff the licensed configuration set is representable as a quotient type (Definition 1.4). If not representable, the judgment of lawfulness degrades to run-time validation (axiom modality ③) or to declaration (modality ④).

**注记 4.6**。商类型在 Rust 的稳定类型系统中不可完备表达（第 5 节）；设备树以"列表 + 运行期校验"落实，正是定理 5.1 中降级编码的实例。

**Remark 4.6.** Quotient types are not fully expressible in Rust's stable type system (Section 5); the device tree realizes them as "list plus run-time validation", an instance of encoding degradation in Theorem 5.1.

---

## 5. 三归宿定理 / The Three-Destination Theorem

**定理 5.1（三归宿 / Three Destinations）**。设 x ∈ A ∖ T（代数合法，类型系统 T 不可完备表达），且 x 需落实为可执行代码。则落实方式恰为下列三类之并，且三类互斥：

(D1) **接缝声明 / Seam Admission**：引入不可验证假设 h；x 以 h 为前提进入系统；h 的职责由边界持有者承担，不于内核验证（定义 1.5、1.6）。

(D2) **降级编码 / Encoding Degradation**：以表示物 r ∈ T 替代 x，并附加运行期校验或约定 c，使在 c 的约束下 r 的行为与 x 一致；T 不承载 x 的不变式。

(D3) **类型强化 / Type Strengthening**：将 T 扩张为 T′ ⊋ T（const generics、GAT、外部验证器），使 x ∈ T′；T′ 为有限闭包，故存在 x′ ∈ A ∖ T′。

**Theorem 5.1 (Three Destinations)**. Let x ∈ A ∖ T (algebraically lawful, not fully expressible in type system T), and let x require realization as executable code. Then the ways of realization are exactly the union of the following three classes, which are mutually exclusive:

(D1) Seam Admission: an unverifiable hypothesis h is introduced; x enters the system conditional on h; responsibility for h is held by the boundary owner and is not verified within the kernel (Definitions 1.5, 1.6).

(D2) Encoding Degradation: a representative r ∈ T substitutes for x, with a run-time check or convention c attached, such that under the constraint c, the behavior of r agrees with x; T does not carry the invariant of x.

(D3) Type Strengthening: T is extended to T′ ⊋ T (const generics, GATs, external verifiers) so that x ∈ T′; T′ is a finite closure, hence there exists x′ ∈ A ∖ T′.

**证明**。穷尽性：任何落实要么验证 x 的不变式，要么不验证。不验证 ⇒ (D1)（不验证即声明）。验证 ⇒ 验证发生于类型层或运行期：类型层 ⇒ 原 T 不能承载，须扩张为 T′ ⇒ (D3)；运行期 ⇒ 以 r ∈ T 表示并在运行期校验 ⇒ (D2)。互斥性：三类以验证位置区分——(D1) 无验证责任，(D2) 运行期验证，(D3) 类型层验证；三个位置两两不同。∎

**Proof.** Exhaustiveness: any realization either verifies the invariant of x or does not. Not verifying ⇒ (D1) (not verifying is declaring). Verifying ⇒ verification occurs at the type layer or at run time: type layer ⇒ T cannot carry it without extension to T′ ⇒ (D3); run time ⇒ r ∈ T with run-time validation ⇒ (D2). Mutual exclusiveness: the three classes are distinguished by verification site—(D1) carries no verification responsibility, (D2) verifies at run time, (D3) verifies at the type layer; the three sites are pairwise distinct. ∎

**注记 5.2**。三归宿不是临时处置，而是边界处的理论必然：每一接缝点，系统必选其一。axiom 的既有选择：不可判定不变量（Moore 判定）→ 模态 ④ 声明，即 (D1)；∃ 绑定 → Conforms 类型化商，即 (D3) 的既有实例；效应、非确定、时间、擦除 → 物理层，即 (D2)/(D1) 的运行期落点。

**Remark 5.2.** The three destinations are not ad hoc measures but a theoretical necessity at boundaries: at every seam point, a system must select one of them. The existing choices of axiom: undecidable invariants (Moore judgment) → modality ④ declaration, i.e., (D1); existential binding → typed quotient via Conforms, i.e., an existing instance of (D3); effects, nondeterminism, time, erasure → the physical layer, i.e., the run-time locus of (D2)/(D1).

---

## 6. 双层信任架构 / Two-Layer Trust Architectures

**定义 6.1（双层信任架构 / TLTA）**。二元组 (K, J) 构成双层信任架构，若：K 为小型内核，携带不变量集 I_K；系统内一切操作要么在 K 内得到验证，要么经单一接缝 J 准入；经 J 准入的操作不满足 I_K，其责任以声明方式由外围持有（定义 1.5）。

**Definition 6.1 (Two-Layer Trust Architecture)**. A pair (K, J) is a two-layer trust architecture if: K is a small kernel carrying an invariant set I_K; every operation in the system is either verified within K or admitted through the single seam J; operations admitted through J do not satisfy I_K, and their responsibility is held externally as declarations (Definition 1.5).

**命题 6.2（边界同构 / Isomorphism of Boundaries）**。Rust 的安全/非安全边界与 axiom 的抽象层/物理层边界均为双层信任架构；二者的接缝准入集合不同，且极性相反。此架构模式的历史先例为 Edinburgh LCF：小型可信内核 + 无法穿透内核可靠性的外围扩展。

**Proposition 6.2 (Isomorphism of Boundaries)**. The Rust safe/unsafe boundary and the axiom abstract/physical boundary are both two-layer trust architectures; their seam admission sets differ and their polarities are opposite. The historical precedent of this architectural pattern is Edinburgh LCF: a small trusted kernel with extensions that cannot breach kernel soundness.

**证明（对照 / Proof by Comparison）**。如表 6.1 所列，逐轴对照；极性差异由两个方向性论证确立。∎

**表 6.1（对照 / Comparison）**

| 轴 Axis | Rust (safe/unsafe) | axiom (抽象/物理 abstract/physical) |
|---|---|---|
| 内核不变量集 I_K | 内存安全、别名、类型不变量；副作用合法于外壳 | 全函数、纯、静态结构、零依赖 |
| 接缝粒度 Seam granularity | 表达式级：unsafe 块 | 层边界：进入物理层处 |
| 准入集合 Admission set | 内存、别名、UB 词表操作 | 激活、效应、非确定、时间、擦除 |
| 验证责任 | 内核不验 unsafe 块内不变量；由审计、miri 等补 | 核心不验物理层；模态 ③④ 补 |
| 极性 Polarity | 代数语法内嵌机器块：unsafe 嵌入 safe | 机器层承接代数接口：物理层以 PortCell/Conforms 形态被接受 |

**极性论证 / Polarity Argument**。(a) Rust 方向：安全代码为承载形式，unsafe 块为被准入者，以块的形式嵌入承载形式的语法内部；准入方向为内嵌。(b) axiom 方向：核心/抽象层定义接口，物理层为实现者；被准入者必须重新呈现承载方的接口形态（Carrier 实现 PortCell、Conforms），准入方向为装载且受接口约束。内嵌与装载方向相反，极性相反。∎

**注记 6.3**。第 2–5 节确立的边界对象（第 2 节四轴各实例、第 4 节商类型缺席、(D1)–(D3) 各类）构成上述两个准入集合之并集的枚举；每一对象在定理 5.1 的三归宿中恰有一个落点。大型软件对 L2/L3 类边界对象所要求的全部处置，即该映射关系。

**Remark 6.3.** The boundary objects established in Sections 2–5 (the axis instances of Section 2, the absent quotient type of Section 4, and classes (D1)–(D3)) constitute an enumeration of the union of the two admission sets above; each object has exactly one destination among the three of Theorem 5.1. All dispositions required by large-scale software with respect to L2/L3-class boundary objects are exactly this mapping.

---

## 7. 对 axiom 的应用 / Application to axiom

**命题 7.1（四模态 → 三归宿 / Four Modalities to Three Destinations）**。axiom 四模态纪律与三归宿之间存在如下对应：模态 ②（编译期见证）对应 (D3) 类型强化；模态 ③（部署期验证）对应 (D2) 降级编码；模态 ④（声明）对应 (D1) 接缝声明。模态 ①（抽象层语义定义）为 (D3) 之上限之外的基准语义，不属于三归宿。

**Proposition 7.1 (Four Modalities to Three Destinations)**. The axiom four-modality discipline corresponds to the three destinations as follows: modality ② (compile-time witness) corresponds to (D3) type strengthening; modality ③ (deployment validation) to (D2) encoding degradation; modality ④ (declaration) to (D1) seam admission. Modality ① (abstract semantic definition) is the baseline semantics above the ceiling of (D3) and does not belong to the three destinations.

**注记 7.2（开放问题 / Open Problems）**。下列对象按四轴归类，其落实待定，落地前须过 §8.3 封闭性检查清单（须为 1–5 概念的实例）：

- 高阶绑定（态射作值）：位于可定义性与可行性两轴交汇；态射作值通常要求装箱/间接，与零成本冲突，属可行性轴争议。
- 时间作值（time-as-value）：位于 A ∖ T（运行期时钟须直通为 State 数据）；axiom 不立法调度（§8.4）。
- 封闭极小规范 API（derive 生成）：位于 T 内，属工程改进而非新概念；先定最小闭原语集，再造生成。

**Remark 7.2 (Open Problems)**. The following objects are classified by the four axes; their realization is pending, and before realization each must pass the §8.3 closure checklist (each must be an instance of concepts 1–5):

- Higher-order binding (morphism as value): located at the intersection of the definability and feasibility axes; morphism-as-value generally requires boxing/indirection, conflicting with zero cost, hence a feasibility-axis dispute.
- Time as value: located in A ∖ T (run-time clock must be threaded as State data); axiom does not legislate scheduling (§8.4).
- Closed minimal specification API (derive generation): located within T; an engineering improvement, not a new concept; fix the minimal primitive set before generating.

**注记 7.3（行为范畴五结构与准入四轴（细化））**。本节细化 §2 四轴（机器可达性过滤：可定义 / 可实现 /
可行 / 可验证）的**目标侧**结构，不新增公理。语义目标范畴承载五结构——序（因果序）、成本（时空）、容量（界）、
传递（投递语义）、并发资源（交换复合与单属主）；准入侧另有四轴——效应、状态即资源、并发、时间（时间 / 并发
是否入论域），决定"一个新能力是否需要新增概念"。定位：§2 四轴判定对象能否穿过；五结构描述穿过之后进入的
目标范畴长什么样；二者是一条定理的两面，非平行新论。资源幺半群（semantics-constitution D11）是"并发资源"
结构的一等对象化。

**Remark 7.3 (Five Structures of the Behavior Category and Four Admission Axes (refinement))**. This note refines
the target-side structure against which the §2 four axes (machine-reachability: definability / computability /
feasibility / verifiability) filter; it introduces no new axioms. The semantic target category carries five
structures—order (causal), cost (spacetime), capacity (bound), transfer (delivery semantics), and
concurrency-resource (commutative composition and single-ownership). The admission side carries four axes—
effect, state-as-resource, concurrency, and time (whether time / concurrency enter the domain of discourse),
deciding whether a new capability requires a new concept. Placement: the §2 four axes judge whether an object
passes; the five structures describe what the target category looks like once passed; the two are two faces of
one theorem, not a new parallel doctrine. The resource monoid (semantics-constitution D11) is the first-class
objecthood of the "concurrency-resource" structure.

**注记 7.4（为何恰恰是五结构；资源的不可替代性 / Why Exactly Five Structures; the Irreplaceability of
Resource）**。五结构（注记 7.3）的个数**不是数论必然，是"问题类封闭性"主张，未证**。"运行一个形状"强制
回答几种彼此不可互相回答的问题类：序（顺序）、成本（时空）、容量（界）、传递（投递语义）、并发资源（正交于
其余四者的可组合所有权关系）。"故五"意为：这五种问题类的每一种都不能用其余任何一种来回答；只要每一类又彼此
构造得出物理差异，三结构必漏一类。判别例（两两独立变化）：成本 ≠ 容量（大界每消息 O(1) 对 小界每消息 O(n)）；
容量 ≠ 传递（有界不告知某条消息被投递还是被弃，"满/关闭"是界，"到达/被拒/覆盖"是投递语义）；序 ≠ 传递（确定
顺序位置与投递结果独立）。N 只当自愿增加问题类（更强的义务目标）时出现；新结构须带来既有五类都答不了的新问题类
（按注记 8.6 不构成找补）。**第六候选为时间，已被放置迁出行为范畴**：时间不作为目标范畴一等结构，而作为可
升可降的值（直通 State）与退化（运行期时序进④ 声明）；迁出后五为当前封闭数——不是不能有六，是有六时把它
归实例层 / 声明层而从范畴五移走。**资源的不可替代性**：成本 / 容量 / 传递 / 序皆描述"单条接缝 / 单 cell 运行"
的幅度（贵不贵、多深、到没到、先后），资源描述"横跨拓扑的可组合所有权关系"（frame 律 P∗R：两件事不相干 ⟹
可分别验证再总合）。没有资源代数，说不出"这两个并行 cell 可以分开证"这句组合的机械化话——而这是大型组合系统
（愿景）唯一买得到验证可扩展的途径；即便无限容量，资源问题仍在（还能否并行两个不相干 cell，是所有权非缓冲），
故资源与前四者正交。**为何资源涌现得晚**：前四结构在 carrier 分类里早已寄居为属性（成本=CarrierCost、
容量=CAP、传递=投递四态、序=因果流与 T5 等价），只是被重新命名整理；资源无旧居——义务类 D1 只有资源"类"
（ZeroAllocInline / PerMessageAlloc / External）而无一等代数对象，故资源是唯一需造新对象、唯一配称"真新增"
者。**主张（非结论）**：五结构与开放问题 8.2 / 8.4 一同作为收敛点假说登记；资源幺半群仍未代码化，按诚实
纪律保持"未落地"标注。

**Remark 7.4 (Why Exactly Five; the Irreplaceability of Resource)**. The count of five (Remark 7.3) is not a
number-theoretic necessity but a claim of question-class closure, unproved. "Running a shape" forces answering
several question classes that cannot answer one another: order, cost, capacity, transfer, and concurrency-resource
(a composable-ownership relation orthogonal to the other four). "Five" means: each of these classes cannot be
answered by any other, and each is pairwise constructively distinct, so three must miss at least one class
(counterexamples: cost ≠ capacity, capacity ≠ transfer, order ≠ transfer). N appears only when one voluntarily
adds a question class under a stronger obligation goal. The sixth candidate—time—has been placed out of the
behavior category: not a first-class structure but a value threading State or a degeneration declared ④. A
structure-graph stays five because time is moved to the instance/declaration layers, not because a sixth cannot
exist. Resource is irreplaceable: the other four describe the magnitude of a single seam/cell (cost, depth,
delivery, order), while resource describes the composable ownership relation across a topology (frame rule P∗R:
disjoint ⟹ separately verifiable); without a resource algebra one cannot state the mechanized sentence "these
two parallel cells can be proved apart", the only route by which a large compositional system buys verification
scalability. Resource appears late because the other four already live as attributes in the carrier
classification (cost=CarrierCost, capacity=CAP, transfer=delivery states, order=causal flow and T5), while
resource has no prior home—obligation class D1 has only resource *classes*, no first-class object—so resource is
the only one requiring a new object and the only legitimate "genuinely new" item. Claim, not conclusion: the
five, with Open Problems 8.2 / 8.4, are registered as a convergence-point hypothesis; the resource monoid remains
unimplemented, kept untested per honesty discipline.

---

## 8. 结论 / Conclusion

**中文**：本文建立了四轴过滤（命题 2.5）与闭包交集（定理 3.1）两个基础命题，给出合法重接线的语义保持准则（定义 4.1）并证明其二元属性（命题 4.3），证明三归宿的穷尽性与互斥性（定理 5.1），并将 Rust 与 axiom 的边界统一为极性相反的双层信任架构（命题 6.2）。所得结论如下：(1) 软件的"合法"不是一元谓词，而是相对于闭包系统的关系；(2) 代码与代数互不包含，工程落地的对象须同时穿过可定义性、可实现性、可行性、可验证性四个独立过滤器；(3) 对每一个边界对象的处置不是消除边界，而是将对象显式归类于三归宿之一，并使其验证位置（或声明责任）显式化。axiom 的四模态纪律是该结论的一个实例化。

**English**: This paper establishes two foundational propositions, four-axis filtering (Proposition 2.5) and closure intersection (Theorem 3.1), gives the semantics-preserving criterion for lawful rewiring (Definition 4.1) and proves its binary attribute (Proposition 4.3), proves the exhaustiveness and mutual exclusiveness of the three destinations (Theorem 5.1), and unifies the Rust and axiom boundaries as two-layer trust architectures of opposite polarity (Proposition 6.2). Conclusions: (1) software "lawfulness" is not a unary predicate but a relation relative to a closure system; (2) code and algebra neither contains the other, and an object of engineering realization must pass the four independent filters—definability, computability, feasibility, verifiability—simultaneously; (3) the disposition of each boundary object is not the elimination of the boundary but the explicit classification of the object into one of three destinations, with its verification site (or declaration responsibility) made explicit. The axiom four-modality discipline is an instantiation of this conclusion.

---

## 9. 分层律 / The Law of Stratification

> 本节将 §2 四轴、§5 三归宿、§6 双层信任架构之下的隐含元律显式化；§8 结论不受影响，
> 本节为其上层定理化。

**定义 9.1（构造拒绝 / Illegibility）**。设 P 为对象类上的合法谓词。机制 M 以构造拒绝执行 P，当且仅当违反 P 的对象不可构造：受约束文法的良构条件排除它们，不存在独立的检查或准入步骤。检查机制与拒绝机制的区分：前者在对象已存在后判决，后者使对象根本不存在。

**Definition 9.1 (Illegibility)**. Let P be a legality predicate on a class of objects. A mechanism M enforces P by construction refusal iff objects violating P are not constructible: the well-formedness conditions of the constrained grammar exclude them, and no separate check or admission step exists. The distinction between checking and refusal: the former judges an object that already exists; the latter makes the object not exist.

**命题 9.2（机制同一性 / Mechanism Identity）**。Rust 的安全保证与 axiom 的 T1/Conforms 判定是同一机制（构造拒绝）在两个不同平面的实例：前者拒绝值平面（内存/别名）的违规，后者拒绝结构平面（组合/拓扑）的违规。

**Proposition 9.2 (Mechanism Identity)**. Rust's safety guarantees and axiom's T1/Conforms judgments are instances of the same mechanism (construction refusal) on two different planes: the former refuses violations in the value plane (memory/aliasing), the latter refuses violations in the structural plane (composition/topology).

**定义 9.3（分层约束向量 / Stratified Constraint Vector）**。约束空间是各平面约束集 Σₖ 的积 Σ = ∏ₖ Σₖ；每个 Σₖ 是闭包系统（定义 1.1），拥有自己的生成元与算子、自己的合法谓词，以及自己的可判定时间 tₖ（编译 / 部署 / 运行 / 声明）。

**Definition 9.3 (Stratified Constraint Vector)**. The constraint space is the product Σ = ∏ₖ Σₖ of plane-specific constraint sets; each Σₖ is a closure system (Definition 1.1) with its own generators and operators, its own legality predicate, and its own decidability time tₖ (compile / deploy / run / declare).

**命题 9.4（标量递增是投影伪影 / Scalar Increase Is a Projection Artifact）**。"约束总量递增"是 ∑ₖ |Σₖ| 的标量投影；其真陈述是"分层变细"（平面数增加），而非单一轴上的长度累加。跨平面的约束彼此正交，不构成同一度量。

**Proposition 9.4 (Scalar Increase Is a Projection Artifact)**. "Total constraint increase" is the scalar projection ∑ₖ |Σₖ|; the truthful statement is "finer stratification" (more planes), not length accumulation on a single axis. Constraints across planes are mutually orthogonal and do not form a single metric.

**定义 9.5（可判定性阶梯 / Decidability Ladder）**。谱：每个阶 k 拥有一个在其时间 tₖ 可判定的合法谓词；不可判定残余 Rₖ 移向下一阶的接缝。实例（含 axiom 落点）：

| 阶 k | 平面 | 合法谓词 | 判定时间 tₖ | 残余承载 |
|---|---|---|---|---|
| 语法 | 可执行性 | 良构 | 汇编/链接 | —— |
| Rust | 内存/别名 | 借用规则 | 编译（拒绝） | unsafe 接缝 |
| axiom core | 组合/拓扑 | T1 对偶、五概念 | 编译（拒绝） | Conforms 缺失即不可实例化 |
| axiom core | 失败 | step 全函数 | 类型约定 | Out = Result（失败为值） |
| axiom runtime | 环良定义 | Moore | 声明（④） | drive_feedback_inline 门 |
| axiom runtime | 成本 | CarrierCost 序 | 声明/部署（③） | 未声明默认 External |
| 插件 | 许可 | 型位合规 | 编译（Conforms）+ 运行（∃） | SlotDrive 装箱 |

**定理 9.6（分层律 / Law of Stratification）**。对任意阶 k，其合法边界恰为两项之并：(L₁) 在 tₖ 可判定的合法性；(L₂) 显式承载不可判定残余的接缝。且 (L₂) 非空当且仅当该阶存在超出 tₖ 判定力的性质。

**Theorem 9.6 (Law of Stratification)**. For any stratum k, its legality boundary is exactly the union of two parts: (L₁) legality decidable at tₖ; (L₂) a seam explicitly carrying the undecidable residual. And (L₂) is nonempty iff the stratum possesses a property beyond the deciding power of tₖ.

**证明**。(L₁)∪(L₂) ⊆ 边界：对阶 k 的任一性质 Q，要么在 tₖ 可判定（入 (L₁)），要么不可判定（构成残余，须被承载）——两分法穷尽，无遗漏方向。残余的承载形态：显式经接缝准入，或隐性被沉默假定；四模态（②编译期见证 / ③部署期验证 / ④声明）穷尽显式承载的全部形态（能力内无需准入为该四态的余情形）。故 (L₂) 的形态由四模态穷尽，边界二分成立。∎

**Proof.** (L₁) ∪ (L₂) ⊆ boundary: for any property Q of stratum k, either Q is decidable at tₖ (into (L₁)) or undecidable (constituting a residual that must be carried)—the dichotomy is exhaustive with no omitted direction. Carrying forms of a residual: admitted explicitly through a seam, or assumed silently; the four modalities (② compile-time witness / ③ deployment validation / ④ declaration) exhaust the explicit carrying forms (the absence of a need for admission within the stratum's power is the complementary case of the four). Hence the forms of (L₂) are exhausted by the four modalities, and the boundary dichotomy holds. ∎

**推论 9.7（错层是元缺陷 / Misplacement Is the Meta-Bug）**。约束若被放置于其合法谓词在 tₖ 不可判定的阶上执行，则按定理 5.1 必退化为 (D1) 接缝声明、(D2) 降级编码、(D3) 类型强化之一——三者皆为"放置修正"的执行器。元缺陷定义：约束-阶配对错误（约束力超出该阶判定力时，宣称语义与兑现语义分歧）。

**Corollary 9.7 (Misplacement Is the Meta-Bug)**. A constraint executed at a stratum at which its legality predicate is undecidable at tₖ necessarily degrades, per Theorem 5.1, to one of (D1) seam admission, (D2) encoding degradation, (D3) type strengthening—all three are executors of a placement correction. Meta-bug definition: a constraint-stratum pairing error (when constraint power exceeds the stratum's deciding power, declared semantics diverges from realized semantics).

**命题 9.8（平面新颖性 / Plane Novelty）**。Rust 约束值平面；axiom 约束结构平面与时间平面（拓扑合法性、激活、成本、许可）。axiom 的关键增量是把接缝结构本身作为一等对象：成本声明（CarrierCost）、残余定位（载体）、许可类型化（Conforms）、不可判定标注（模态）。

**Proposition 9.8 (Plane Novelty)**. Rust constrains the value plane; axiom constrains the structural and temporal planes (topology legality, activation, cost, licensing). The key increment of axiom is making the seam structure itself a first-class object: cost declaration (CarrierCost), residual location (carrier), licensing typing (Conforms), undecidability labeling (modality).

**注记 9.9（同律投影 / Projections of the Same Law）**。§8.3 的"无第六概念"判据、frontier-notes 第 3 条封闭极小规范 API、概念 1 的尺度中立，均为分层律的投影：新增平面须满足闭包判据；封闭极小 API 使可判定域最大化、接缝面最小化；尺度中立使分层律在系统内部递归成立（子系统是同级 cell）。

**Remark 9.9 (Projections of the Same Law)**. The §8.3 "no sixth concept" criterion, frontier-notes item 3 (closed minimal specification API), and the scale neutrality of concept 1 are all projections of the law of stratification: a new plane must satisfy the closure criterion; a closed minimal API maximizes the decidable domain and minimizes the seam surface; scale neutrality makes the law hold recursively inside a system (a subsystem is a same-scale cell).

**注记 9.10（矛盾分类学 / Taxonomy of Contradictions）**。分层律的经验注记：清晰的定义并不消灭矛盾，而把矛盾从"散落隐含"改写为"定位展出"。三类矛盾的命运各异：(i) **类别矛盾**（构造期不可写、语义不可静默）——被定义**消除**（A1 拒绝 + 四轴分类）；(ii) **边界矛盾**（层间接口、语言表达力；定理 3.1 的闭包差异）——被定义**定位**到接缝，由四模态承接（② 见证、③ 验证、④ 声明，定理 9.6 (L₂)）；(iii) **经验矛盾**（性能、工具链）——被定义**展出**（经验-D 的 E1 界 + 监测）。故被完备封闭的是概念层（构造拒绝区）；实现层的矛盾残余在接缝被展出而非被消灭，这正是"无第六概念"判据只约束概念层、而不禁止实现层新增机制的原因（新机制仍须满足 §8.3 闭包判据）。

**Remark 9.10 (Taxonomy of Contradictions).** An empirical note on the law of stratification: precise definitions do not eliminate contradictions; they rewrite them from "scattered and implicit" to "located and exhibited". The three kinds of contradictions have different fates: (i) **categorical contradictions** (unwritable at construction time, not silently semantic) — **eliminated** by definition (A1 refusal + the four-axis taxonomy); (ii) **boundary contradictions** (inter-plane interfaces / language expressiveness, the closure difference of Thm. 3.1) — **located** by definition at the seam, carried by the four modalities (witness ②, deployment validation ③, declaration ④; Theorem 9.6 (L₂)); (iii) **empirical contradictions** (performance, toolchain) — **exhibited** by definition (the empirical-D E1 bound + instrumentation). Hence it is the conceptual plane that is completely closed (the construction-refusal region); the residual contradictions of the implementation plane are exhibited at the seam rather than eliminated — which is precisely why the "no sixth concept" criterion constrains only the conceptual plane and does not forbid new mechanisms in the implementation plane (new mechanisms still must satisfy the §8.3 closure criterion).

---

## 参考文献 / References

[1] H. G. Rice. Classes of recursively enumerable sets and their decision problems. *Transactions of the American Mathematical Society*, 74(2):358–366, 1953.

[2] M. J. Gordon, A. J. R. G. Milner, C. P. Wadsworth. *Edinburgh LCF: A Mechanised Logic of Computation*. LNCS 78, Springer, 1979.

[3] Devicetree Specification, Release v0.4, devicetree.org, 2023. §2.2 (nodes), §2.3 (properties), §4 (reg encoding).