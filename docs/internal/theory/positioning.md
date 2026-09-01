# 定位：六个透镜下的组合系统论 / Positioning: Compositional Systems Theory through Six Lenses

> **性质**：I1 层定位注记（`docs/internal/theory/`，白名单跟踪，公开仓库）。非新公理、非承诺：
> 本文不对既有规范增加任何定义或定理，只把 [`boundary-ontology.md`](boundary-ontology.md)、
> [`meta-foundations.md`](meta-foundations.md) 与公开规范 `docs/{en-us|zh-cn}/foundations.md`
> 已确立的构件放入六个科学透镜下解读，并登记此前未收录的锚点。冲突时以上述规范为准。

---

## 0. 引言 / Introduction

**中文**：本文回答一个问题：原子组合、系统论、信息论、涌现（经验）论、计算科学，以及
描述它们的逻辑与现代数学，各自能解释本事业的哪一部分。体例：每节一张映射表（透镜概念
→ 既有构件）＋一条边界声明。新增锚点仅两处（§6.1、§6.2），其余均指向既有文献。

**English**: This note maps six scientific lenses onto established constructs. Each section is
a mapping table plus a boundary statement; only two anchors are new (§6.1, §6.2).

---

## 1. 原子组合：组合子逻辑与项代数 / Atomic Composition

| 透镜概念 | 既有构件 |
|---|---|
| 生成元（combinatory basis） | 五概念封闭集（foundations §8.1）；`PortCell` = (S, I, O, δ) |
| 项代数与初始语义 | 蓝图＝代数表达式＝一个类型；[`closed_boundary`] 测试＝初始性检查 |
| 基不唯一（等价基交换） | 完备性诊断两类归因（theory-archive §2.2）：补生成元 vs 回溯公理 |
| 组合封闭（SKI 之"组合后仍是系统"） | T2（组合构成操作类结构） |

**边界声明**：原子性是生成元集的选取决定，不是被发现的事实；不同基可生成同一空间，
故"五个"是构成事实而非必然真理（meta-foundations 定义 1.8 的基选取问题）。

---

## 2. 系统论 / Systems Theory

| 透镜概念 | 既有构件 |
|---|---|
| Bertalanffy 开放系统（以组织定义系统，交换越界） | 开放系统/操作类（foundations §1.0；Fong & Spivak 2015） |
| Ashby 必需多样性定律 | 接缝类型界定跨界变差；投递四态（delivery.rs）＝载体的变差分类学 |
| Boulding 系统复杂度层级（1–9 级） | 1–5 级五概念可表达；8–9 级（社会/超验）出域——剖面机制（profile.rs）即诚实收缩 |

**边界声明**：必需多样性给出的是义务语法存在的必要条件，不是充分条件；变差的完全枚举
受 Rice 边界约束，剩余部分落模态④。

---

## 3. 信息论 / Information Theory

| 透镜概念 | 既有构件 |
|---|---|
| 有型信道（类型界熵上界） | `Wire<A,B>` 配对（T1） |
| 信道容量有限 | `CAP` 编译期常量（L3）⇒ 背压是信息论必然，非工程偏好 |
| 离散擦除信道（擦除符号带内显式） | `Delivery<T>`：Full/Closed 为带内擦除符号，值随判定回传；静默丢失被逐出 |
| 共享介质噪声管理代价 | 成本族 A（并发维护税：同步/唤醒/内存序） |
| 可区分性编码开销 | 成本族 B（区分需求成本，设计级可消除） |
| 描述长度不变性（Kolmogorov） | 零成本相对等式：产物不含超出手写等价物的描述膨胀 |

**边界声明**：Shannon 界针对通信；计算侧的对应（算法信息论）只支持"相对等式"级别的
陈述，不承诺全局最短描述（foundations 边界条款同款保留）。

---

## 4. 涌现（经验）论 / Emergence and Empiricism

| 透镜概念 | 既有构件 |
|---|---|
| 弱涌现（可推导的惊喜）vs 强涌现（拒绝） | 结构面完全构造性；行为面受 Rice 边界约束 |
| Anderson 尺度律（More is Different） | 族 A 成本跨线程放置下尺度不可消——以经验-D（E1）入账，非公理 |
| 不可判定带的围栏 | 谱分层（boundary-ontology §9 递归应用）：每阶层只承认其谓词可判定者，余者模态④ |
| 纲领的经验迭代 | 审计史＝Lakatos 进步/退化判据的时间实践（meta-foundations 命题 8.4） |

**边界声明**："结构零涌现、行为有涌现"是一个工作假定，不是定理——其可否证形式为：
若某结构性质既不可由五概念推导又不可判定，则须新增构造或修订公理（封闭清单程序）。

---

## 5. 计算科学 / Computing Science

| 透镜概念 | 既有构件 |
|---|---|
| 普遍性（Church–Turing）⇒ 区分度在描述代价 | 零成本承诺：表达代价相对手写守恒（T7 的工程形态） |
| 判定时间的复杂度分层 | tₖ 阶梯：编译 < 部署 < 运行 < 声明（meta-foundations 定理 3.1） |
| 机制/政策分离（OS 传统） | L4 政策归驱动：cell 无时间语义，拍次只在 driver/carrier |
| 端到端论证（放错层的功能无法正确） | 推论 9.7（错层是元缺陷）的同构；义务落位律为其机械形式 |

**边界声明**：端到端论证是设计准则（启发式），错层判定是元层面陈述；二者同构但不互证。

---

## 6. 逻辑与现代数学：存量索引与两处增量 / Logic and Mathematics

### 6.0 存量（已收录于既有卷，此处仅索引）

操作类与 Wiring 图（Fong & Spivak 2015）；会话类型线性配对（Honda；Wadler 2012）；依值
类型论；共代数；层论/透镜（foundations §1）；闭包系统与格（boundary-ontology；
meta-foundations）；帕累托偏序与不动点（meta-foundations 猜想 8.1、结论）。

### 6.1 增量一：反馈的数学形态（听证 D 修正后）

**裁定基线（听证 D / 修正 12.10）**：`Feedback<BODY, FEED>` 的被裁决形态是
**守卫反馈**——单元形式固定一次内联闭合迭代、FEED 侧一拍延迟（外部端口不变、
内部线 U 绕回经一拍守卫），yanking（无延迟即取）是断言对象（负见证：守卫
反馈 ≠ 即取交换，laws.rs `yanking_fails_under_guarded_ruling`）。
**"迹算子 = Feedback 的数学形态"这一强主张不取**：迹的不动点语义（Trace
Joyal–Street–Verity 1996）与一拍守卫的语义序不同构；修正 12.10 后幸存的可说
内容收缩为**部分迹等式**（消失/连接/叠加/张紧）与结构形态的有限对应，
非形态等同。缓冲环对应"其它展开策略"的说法随之降级为工程对照，非同一定律
的展开变体。SCADE/Lustre 传统的反馈方程仍是守卫反馈的工程先例（一拍延迟
与同步数据流的拍次语义一致）。

### 6.2 增量二：模态体系的两种逻辑读法

- **认识论读法（BHK 解释）**：①②③要求见证（构造证明），④是未证断言——模态格
  {①②③④} ∪ {∅} 是证据强度的序；∅（未展出的隐含）为违例零点（meta-foundations
  开放问题 8.3）。
- **义务论读法（标准语）**：RFC 2119 MUST/SHOULD/MAY 是另一支模态逻辑；SHOULD 的
  "偏离须附文档化理由"即诚实规则。两轴正交（实现标准化命题 7.4 的审计结论）。

---

## 7. 综合命题 / Synthesis

**概要**：本事业＝给 Bertalanffy 的组织之问一个 Church–Turing 有效的答案——一个封闭
组合基，其项是编译期可判定的形状（范畴论），缝携带 Shannon 可读的契约（类型化擦除
信道），证据主张按直觉主义见证纪律分级（模态格），不可约动力学被围入声明区与
经验区（涌现围栏），成本服从相对手写的守恒律（描述长度框架）。

**三条可检验推论**（与既有判据一致）：

1. 新需求以实例而非修宪落地 ⟺ 统一持续成立；
2. 凡同构者必复用（极小基律的自指执行）；
3. 反例预警：Gabriel "worse is better"——完备统一历史上屡被够用的简单取代；剖面机制
   （承认每类软件自己的物理）是对该规律的吸收，不是豁免。

## 8. 层级—压缩—涌现的数学谱系 / Mathematics of Hierarchy, Compression, Emergence

用户之问："顶层基生成任意复杂系统、复杂度阈值处涌现需新描述的现象、正确的定义压缩
复杂性——是否存在这样的理论与数学表达？"存在一个理论星座（非单一理论）：

| 直觉 | 数学对应 | 先行学科 |
|---|---|---|
| 顶层基 ⟹ 任意项树 | 自由操作代数/初始代数；Simon 近可分解层级（1962） | 组合逻辑；系统论 |
| 涌现之"谱" | 重整化群粗粒化的相关/无关算子谱（Wilson 1971）；复杂度–熵峰带与 ε-机器统计复杂度（Crutchfield 1989；Langton 1990） | 统计物理；计算力学 |
| 正确描述压缩复杂性 | 两部码/Kolmogorov 结构函数与 sophistication（Vereshchagin–Vitányi 2004）；MDL（Rissanen 1978）；Solomonoff 归纳 | 算法信息论 |
| 涌现的代数化 | 余极限不保持分量结构＝胶合产生新对象（Goguen 1973/1991）；因果涌现 ΔI>0（Hoel 2017）；记忆演化系统（Ehresmann–Vanbremeersch 2007） | 范畴论一般系统论 |
| 各层需自己的描述 | Oppenheim–Putnam 统一科学（1958）vs Fodor 特殊科学多重可实现性（1974） | 科学哲学 |

**对本事业的映射**：T6 多物理实现定理＝多重可实现性的工程翻译；剖面与模态④＝特殊
科学自治权的受控版本；落位律＝两部码的手工执行（最强见证模态＝最省模型）；tₖ 阶梯与
剖面＝手工的重整化粗粒化（每阶层保留其可判定谓词，声明其余）。`PortCell` 的 S 即
ε-机器意义上的极小预测状态。

**边界声明**：上述星座互不归约；本事业选取的立场是"弱涌现＋显式声明围栏"。若某结构
性质既不可由五概念推导又不可判定，封闭清单程序即修正通道——该立场的否证条件与 §4
一致。

---

## 参考文献 / References（增量部分）

[1] M. Schönfinkel. Über die Bausteine der mathematischen Logik. *Mathematische Annalen*, 92:305–316, 1924.

[2] L. von Bertalanffy. *General System Theory*. George Braziller, 1968.

[3] W. R. Ashby. *An Introduction to Cybernetics*. Chapman & Hall, 1956.

[4] K. E. Boulding. General Systems Theory—The Skeleton of Science. *Management Science*, 2(3):197–208, 1956.

[5] C. E. Shannon. A Mathematical Theory of Communication. *Bell System Technical Journal*, 27:379–423, 623–656, 1948.

[6] P. W. Anderson. More Is Different. *Science*, 177(4047):393–396, 1972.

[7] A. Joyal, R. Street, D. Verity. Traced Monoidal Categories. *Mathematical Proceedings of the Cambridge Philosophical Society*, 119(3):447–468, 1996.

[8] J. H. Saltzer, D. P. Reed, D. D. Clark. End-to-End Arguments in System Design. *ACM Transactions on Computer Systems*, 2(4):277–288, 1984.

[9] M. Li, P. Vitányi. *An Introduction to Kolmogorov Complexity and Its Applications*. Springer, 1997.

[10] R. P. Gabriel. Lisp: Good News, Bad News, How to Win Big (Worse Is Better). 1991.

[11] H. A. Simon. The Architecture of Complexity. *Proceedings of the American Philosophical Society*, 106(6):467–482, 1962.

[12] P. Oppenheim, H. Putnam. Unity of Science as a Working Hypothesis. *Minnesota Studies in the Philosophy of Science*, 2:3–36, 1958.

[13] J. A. Fodor. Special Sciences (Or: The Disunity of Science as a Working Hypothesis). *Synthese*, 27:97–115, 1974.

[14] K. G. Wilson. Renormalization Group and Critical Phenomena I. *Physical Review B*, 4(9):3174–3183, 1971.

[15] J. P. Crutchfield, K. Young. Inferring Statistical Complexity. *Physical Review Letters*, 63(2):105–108, 1989.

[16] N. K. Vereshchagin, P. Vitányi. Kolmogorov's Structure Functions and Model Selection. *IEEE Transactions on Information Theory*, 50(12):3265–3290, 2004.

[17] J. A. Goguen. A Categorical Manifesto. *Mathematical Structures in Computer Science*, 1(1):49–67, 1991.

[18] E. Hoel. When the Map Is Better Than the Territory. *Entropy*, 19(5):188, 2017.
