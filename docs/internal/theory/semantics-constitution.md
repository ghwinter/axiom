# semantics 宪法：义务代数的落地设计 / Semantics Constitution: The Obligation-Algebra Design

> **性质**：I1 层设计规范（`docs/internal/theory/`，入 git）。**本文是 axiom-semantics 破坏性
> 重构的蓝图**：由 [`meta-foundations.md`](meta-foundations.md)（公理/定义/代数）向代码投影。
> 上游：meta-foundations 定义 1.1–1.11、命题 7.4（六元组）、boundary-ontology 定理 9.6、§8.3 封闭判据。
> 状态：Δ 执行中；核心冻结（`core/src/cell_core.rs` 语义零改动）。本文原名 runtime 宪法
> （Runtime Constitution），随 runtime→semantics 层位更名更名，其承继关系记于 theory/README 超驰映射。

---

## 前置：层位身份 / Layer Identity

> 本段定位了本文所辖层位（语义层）在 axiom 三构件中的身份。其中未证部分显式标注为
> **主张（非结论，冲突以规范为准）**，不作为既有规范命题使用。

**三构件**。axiom 分为三个构件，单向依赖（workspace 强制 `axiom ← axiom-semantics ← axiom-instances`）：
- **core（形状 / Shape）**：核心 `cell_core` 冻结。只声明因果数据流（`A.out → B.in`）的**形状**：类型、型位（typed hole）、三绑定态、T1 布线合法性。承载逻辑-D 结构不变量（L1 无静默丢失 / L2 单属主 / L3 容量归位 / L4 政策归驱动）。
- **semantics（语义函子 / Semantics Functor，本层）**：把核声明的形状解释为"在执行基座上跑起来意味着什么"——值如何流动、以何种时空成本、边界条件如何定义。语义函数是映射 **⟦core shape⟧ : Shape → Behavior**（boundary-ontology 的 σ 的范畴级推广，见 §4/§9）。
- **instances（实例层 / Bindings）**：绑定爆发——具体执行基座（tokio / io_uring / std 世界）接入语义接缝，兑现等待点与真实事件源。

**语义函数与 T6（主张）**。语义函数 ⟦·⟧ 把形状范畴映到行为范畴：同一形状的多物理实现（载体）因"对应同一行为"而等价——即 T6（多物理实现语义等价）。**T6 = 语义函数的同余 / 自然性，为主张非已证结论**；其可靠地位不用于证明区，只作语义层的组织原则。

**基座优先（substrate-first）**。并发 / 异步是语义基座：复杂软件本征地跑在多核多线程，模块分布多执行序是基例、非例外。**单线程同步纯单元 = 该基座的退化极限**（一个执行序、无交错、无等待），不是并发随后附加的原始态。无共享全局时钟时，不存在"普遍同步情形"可供一般化。故本层把异步 / 并发当作语义基座，同步纯形态按其退化极限表述。

**no_std = 抽象层纯承诺**。core 与 semantics 以 `no_std` 交付，是"零成本抽象"的**另一面**：形状层不依赖宿主运行时，是可移植的纯承诺（模态①的物理侧）。不把 no_std 当作终点或特权，只当抽象层交付形态的一个约束。

**std 可替换**。实例层支持任意绑定世界（`std`、tokio、io_uring……），**std 本身可替换、无特权**：它是具体绑定世界之一。唯一不可替换的底是语言核心与 `alloc`；接缝（socket）一律按**能力声明**（capabilities，如 `Executor` 可替换等待点、`Carrier` 可替换传输、`Telemetry` 可替换观测），不按 `std` 具体类型立约。能落到能力层的绑定，落能力层；落不到者（绑定爆发、探针实现）归实例层。

**本层不执行**。semantics 承载契约与接缝，不执行任何东西：等待点、事件源、真实绑定的兑现归实例层（§11）。执行位置低移，不改变语义拓扑（T6 辖域内）。

---

## 1. 公理集 / Axioms

- **A1 构造拒绝**：良构即合法，违规不可构造（T1/`Conforms`，类型层，模态①）。
- **A2 三分区**：每层义务 ∈ 文法区 ∪ 证明区 ∪ 公理区；合法边界 = 可判定合法性 ∪ 显式残余接缝（boundary-ontology 定理 9.6）。
- **A3 落位律**：义务必须置于其见证形态能支撑的最强模态（meta 定义 1.10）。放弱浪费可判定性；放强退化为伪精确。
- **A4 极小基律**：公理区不含可由其余成员加规则导出的成员（meta 定义 1.9）。违例 = 伪验证缺陷。
- **A5 诚实规则**：公理区成员必须展出，不得伪装为证明区成员（meta 定义 1.5）。
- **A6 外审**：可靠性不能自证；须外部审查（测试/审计/符合性件）（meta 命题 5.1）。

## 2. 定义集 / Definitions

- **D1 义务类**：参数化谓词族 = 投递态（Full/Closed/Timeout/Cancelled）× 资源类（ZeroAllocInline/PerMessageAlloc/External）× 引用有效（代戳） × 生命周期（许可阶段）。
- **D2 模态**：① 结构见证（类型级）/ ② 常量见证（编译期）/ ③ 部署验证（装配期）/ ④ 声明；模态格 {①②③④} ∪ {∅（违例）}，每条义务恰占一格。
- **D3 接缝**：两层之间的准入通道；残余必须被显式承载。
- **D4 载体**：物理实现；须声明成本（`CarrierCost`）与义务类。
- **D5 semantics**：义务代数的机械——物理层自分层（分层律在物理层内的递归应用）。本文原名 runtime
  宪法称 D5 为 runtime；随层位更名更名，语义不变。

**定义卷（并入自 internal 工作稿；见 theory/README §2 理论并编承接）**。以下 D6–D11 补全语义负载词首的
定义缺口，按 theory/README §3 文体重写。冲突以公开规范为准。

- **D6 语义 / Semantics**：从配置空间到可观察行为的映射 σ: W → Obs（boundary-ontology 定义 1.2），连同在该
  映射上定义的关系——合法重接线（σ∘ρ = σ）、行为等价（T5）、语义保持。语义是序对（载体, 语义）的一个分量
  （boundary-ontology 命题 4.3）。**边界（与类型判定互补）**：类型判定在分层某阶 tₖ 可判定者归 ①②③；语义
  性质的**真值不在任何 tₖ 被系统验证**者只允许 ④（meta-foundations 定义 1.4）。**推论（语义不含物理发生）**：
  语义不包含物理事件的发生机制，只包含语义函数把它映成何种可观察行为。
- **D7 IO 二分**：axiom-IO = 信息传递（抽象层只断言"类型 + 因果"，机制与时机归物理载体，foundations §1.0a）；
  OS-IO = 外部世界事件的物理发生（socket / 文件 / 信号；非确定、可失败 / 阻塞 / 有副作用）。**接缝**：OS-IO 经
  载体成为 axiom-IO 的 in；该接缝是"未判定残差的物理层落点"，按三归宿落实于模态 ③④，**须显式声明**，否则即
  未形式化洞（原名 runtime.md §9.3 之承认）。**边界（失败归属）**：axiom 不承诺统一 OS 错误的语义；axiom-IO
  拥有的是传播纪律（fail-closed、no-silent-loss，本文宪法）；OS 上"每次失败 / 阻塞 / 副作用意味着什么"归
  处理器 / 载体定义，不属 core。
- **D8 观测 / 观测层**：观测 = 读取可观察行为（σ 的余象 Obs 的成分）；观测**不是第三种流类**——观测值 / 控制值 /
  数据值物理同物（foundations §5.8），"观测"只是"数据被消费在别处"的位置描述，非独立语义原语。观测层 = 承载
  观测接口（收集 → 输出 / 日志 / 审计）的抽象层接口，是构造概念 4 的一部分，**非第六构造概念**。**边界（诚实
  落点）**：观测可到达接口与载体判定的可观察行为（S），**不进入 cell 内部 `State`**（`State` 按遗漏无结构，
  core.md §2.1）。**推论**：观测 `State` 内部演化是给 `State` 增加结构的后果（见 D11 资源幺半群），非观测层
  默认能力。
- **D9 step 边界与卡死放置权**：`step : State×In → State×Out` 是组合节点的（单跳、同步）边界契约——钉住
  "线缆两端这一跳的输入输出关系"，如同 C 函数签名只描述边界、不捕获内部载体。`step` 的"纯"非计算本性主张，
  而是"边界不变量在流动不卡死片段上成立"的约定（决策 T；core §2.1）。**卡死放置权二选一**：(a) 归载体
  （实现现状：卡死放置于队列 / 载体 / async_seam，`step` 保持全函数，core 承诺"step 不卡死"）；(b) 归行为
  范畴（若要"卡死本身"作原子语义，原子单位是事件 / 握手，非 step，落并发基座 8.6 item9）。二者非定义层冲突，
  是放置层选择；连续性流 / 反馈本非 step 形（流需共归纳、反馈需 trace，均落语义目标范畴）。
- **D10 接口 / 组合不透明 / 并发 / 异步**：接口 = 一个节点暴露的、受类型的端口集合的可用子集；连接经线的合法性
  由 T1 判定；接口是"组合不透明"的约定边界，**不是"不可观察"的物理墙**。**弃用"黑盒"**，代之以二分：
  (a) 组合不透明（只承诺接口、内部可换，条件于接口保持）；(b) 按遗漏无结构（`State` 无声明结构，可经接口演进
  加结构）。"黑盒"的"不可观察"暗示与 D8 观测平凡化矛盾，故不采用。并发 = 行为范畴中可交换复合（对齐 CKA ‖）
  与事件 / 握手的原子性；它是**基座**，非退化附加。异步 = 可由挂起 / 恢复暂停的交互原子，是并发基座的可暂停
  实例。**边界（同步退化）**：单线程同步 = 该基座的退化极限；实现若以同步纯 cell 为默认原语、异步置于 feature
  门后（现状 `no_std` 单线程优先 + `async-seam` 门），记为基座优先的**差距**，非满足项。
- **D11 资源幺半群（唯一真新增）**：把资源携带到交换幺半群 $(M, \cdot, \varepsilon)$：$M$ = 全部可构筑资源，
  $\cdot$ = 资源合并（顺序无关、可分可并），$\varepsilon$ = 空资源；acquire = 从共享池取出、release = 放回、
  "持有" = 状态里携带的那份资源。**frame 律**：分离逻辑的 frame 规则给出"资源可分开推理"——$R$ 与 $P$ 分离
  （$P \ast R$）时，$C$ 的论断对 $R$ 无需改动，是组合验证可分解（T4）在资源维度的机械，补 D1 缺失的"可组合
  资源对象"。**边界（落于行为范畴，不改 core）**：资源幺半群是行为范畴的一等对象（同"成本在热带半环"落语义函子
  侧），不修改 core 的 `State: Default` 形状，是对 `State` 结构约束的扩充；与其 L2 单属主（`&mut` 独占）相容
  ——单属主是 frame 律所需的不相交所有权。**诚实边界**：这是语义语料**唯一真正的扩展**（其余 D6–D10 均为
  既有理论的重命名或补齐），故标"新增"；其可靠依赖"资源不相交"假说，跨结点一致性不在此层。

**边界（容量归位，收窄 L3；失败层级中的容量落位）**。容量是随放置而变的属性，非形状属性；扩容与重启
视角等价——二者同属生命周期聚合操作，容量由聚合时刻的调用者选择，不刻进蓝图。由此 L3 界值先验作措辞
收窄：(a) 抽象层纯承诺子集（无堆、无运行时可校验）且携带投递保证的有界接缝，容量须编译期常量且 ≥ 1，
为模态② 见证；此为该子集唯一能取得"不静默丢失 / 有界等待"保证的机制。(b) 一切"计算图上的未来"对象
（动态 / 插件 / 外部 / 跨机器 / 可重启 / 可扩容），其容量在聚合时刻才可知，归部署期校验（模态③）或由
开发者与使用者声明（模态④）。判定纪律：不得把未来对象的容量写成 ②；把"聚合才决定"写成"编译期已知"
即伪验证（诚实规则 A5 违例）。失败层级中，容量的落位随之分列：静态子集占 ②，未来对象占 ③ / ④。

## 3. 代数 / Algebra

- **义务组合律 f(X,Y)**：载体 C₁（类 X）⊙ 载体 C₂（类 Y）→ 复合义务类 f(X,Y)；f 的定义与健全性证明是工程项（A 若满足 X、B 若满足 Y，则 A⊙B 满足 f(X,Y)）。
- **模态格**：② < ① 不作比较；③ 依见证形态；∅ 为违例零点；每条义务恰占一格，否则构成失效（开放问题 8.3 裁定）。
- **六元组标准化 (S,L,T,C,V,R)**（meta 命题 7.4）：载体目录的发布形态——接口契约 S、规范强度 L、符合性测试 T、剖面 C、版本 V、治理 R。

## 4. 代码投影 / Code Projection（新结构）

```
semantics/src/
  contract.rs   义务账本：义务类 × 模态 × 见证 fn × 测试（A4/A5/A6 的机械）
  obligation.rs 义务类类型系统：DeliveryState / ResourceClass / ReferenceValidity / LifecyclePhase
  delivery.rs   投递四态：Full/Closed 机械化（②③），Timeout/Cancelled 声明（④，机械化为物理选择）
  slot.rs       typestate 生命周期：SlotPending → SlotLive → retired；Seat 代戳（模态①）
  mailbox.rs    有界邮箱反饥饿：容量 = buffer + 每生产者席位；三投递模式 fire/try/block
  event.rs      事件基座载体类：EventStream + ChunkSource + pump_events（§9.3 从首案例到载体类）✅
  flow.rs       驱动：drive_link / assemble_link|seam / drive_seq / drive_try / TryChain / drive_feedback_inline
  buffer.rs     BoundedQueue（对齐 DeliveryState）
  carrier.rs    Carrier trait + Inline/Queue/Bounded + ResultCarrier/MaybeCarrier
  static_path.rs / macros.rs / lib.rs（prelude 按新结构导出）
```

> **门控注（结构收敛 2026-08）**：接缝载体族按细粒度特性门控——`event` /
> `async-seam` / `telemetry`（+ 伞默认 `default = ["std", "event", "async-seam",
> "telemetry"]`）；核心机制族（carrier/contract/obligation/flow/profile/slot/
> delivery/law）与物理原语族（buffer/ring/mailbox）永不被 feature 关。
> 纪律：账本探针引用符号与其模块同门控；CI 特性矩阵见 ci.yml。

## 5. 破坏性 API 变更清单 / Breaking-Change List

1. `SlotDrive` typestate 化：`install → SlotPending`，`commit() → SlotLive`；`retire()` 终结；未 commit 不可驱动（模态①，零运行期检查）。
2. `SlotLive` 提供 `Seat`（借用的驱动视图，携带代）；`swap` 后旧 Seat 以代校验拒绝（过期引用 = 类型/运行期可检错误）。
3. `BoundedQueue::push/try_push` 对齐 `DeliveryState`（Full/Closed 显式区分，值随错误回传）。
4. 载体目录按六元组文档化（每个载体一节：S/L/T/C/V/R）；`ResultCarrier`/`MaybeCarrier` 加入。
5. `bounded_pump` 内部换用 `mailbox`（语义不变，实现换底）或并存（教学形态保留）。

## 6. 执行阶段 / Execution Phases

1. ✅ 契约层：obligation.rs + delivery.rs + contract.rs 账本与落位律测试（A3–A6 机械）。
2. ✅ 生命周期层：slot.rs typestate + Seat 代戳。
3. ✅ 背压层：mailbox.rs + bounded_pump 并存（教学形态保留）。
4. ✅ 事件层：event.rs（EventStream/ChunkSource/split_lines/pump_events 载体类）+ redis_like 实例化（server.rs 已改由 pump_events 驱动；§9.3 收口）。
5. ✅ 短路载体：ResultCarrier/MaybeCarrier（§9.2 收账）。
6. ✅ 示例健壮性：netpath/mmо Result-ify。
7. ✅ 终验：tests / no_std / clippy -D warnings 全部通过 + semantics.md en/zh 同步（本地全量验证：顶层 32 + semantics 59 测试、benches 编译、no_std 双 crate、clippy `-D warnings` 双 crate、docgate 门、en/zh §9.3 同步，全部零发现项）。

每步过 §8.3 封闭判据（无第六概念）：义务类=概念1 失败为值的展开；生命周期=概念4 型位的实现；邮箱/事件=概念5 物理载体的实例；账本=契约（§8.4 物理层义务）。

## 7. 兼容性与开放接缝 / Compatibility and Open Seams

> 命题（接缝契约论 / Seam-Contract Thesis）：与既有生态的兼容由**接缝契约 + 义务声明 + 机制自由**三者承担，不需牺牲拓扑或语义。

- **接缝契约（S）**：`Carrier` trait 的接口与可观察行为（§4 投影）。凡实现 S 者，无论内部机制，皆可入链——库、自研原语、外部载体一视同仁。
- **义务声明（L/C）**：六元组中的规范强度（L）与剖面（C）由载体自行声明（D2 模态、D4 载体、meta 命题 7.4）；符合性（T）由外部测试确立。兼容的代价以义务陈述支付，不以机制绑定支付。
- **机制自由**：约束是声明而非机制——实现可自由选择自研原语或既有库，只要声明相应义务类。

**三处边界（非冲突，已声明）**：

1. **同步 flow 签名 vs 真异步库**：`drive_link` 等为同步驱动；真异步库（async 生态）经异步接缝接入（`AsyncCarrier`，D2 已裁定；接缝源码见 `semantics/src/seams/async_seam.rs`——设计文书未入 docs，正式化时随接缝 prose 迁入），义务层（L/C）保持兼容，拓扑不变。
2. **自研原语 vs 成熟通道库**：`mailbox`/`BoundedQueue` 自研原语与成熟通道库等价竞争——库 = 一个实现 S 的 Carrier + 义务声明（六元组化）；替换不改变链拓扑（T6 等价类）。
3. **剖面预算 vs 库自由**：`assemble_profile` 施加预算门（Kernel/Service/Tool）；预算约束的是声明，不是机制——库在 ToolProfile 下宽松、在 KernelProfile 下被义务 ②③ 见证，语义不变。

**与早期声明的兼容矩阵**：

| 早期声明 | 本文机制 | 关系 |
| --- | --- | --- |
| §4 适配器不改拓扑 | `assemble_profile` 换预算门不动链结构 | 兼容（T6） |
| §2.3 三绑定态（Slot/Wire/SlotDrive） | `SlotPending → SlotLive` typestate | 概念名保留，实现升级 |
| §8.4 政策归驱动 | 剖面预算归装配（③） | 兼容（驱动层不加义务） |
| 失败态分类（Full/Timeout/Cancelled） | `Timeout/Cancelled` 保持声明 ④ | 一致（异步载体落地前不宣称 ②③） |

**判据**：任何新接缝须满足 §8.3 封闭判据——不引入第六概念（库接入 = 载体实例化，非新概念）。

## 8. 控制与观测（语义澄清与共形）/ Control and Observation

> 背景：外部审计（2026-08，tmp2.md）引入"控制面/观测面"，与系统内语义存在双义；
> 本节把两义钉死，并给出 axiom 的共形（不引入新概念，§8.3）。

- **观测面（两语义一致）**：被观测信息的收集 → 输出（控制台/日志/持久化）；审计补充
  "标准 span/counter/event 接口、可 no-op"。落位：**物理层遥测接口**——每边通过数/
  失败数/队列深度/延迟直方图；已有伏笔：事件基座配对计数（`event.rs`）、律探针
  （`law.rs`）；规范缺口 = 标准观测接口与输出目的地（遥测落位，台账 B1）。
- **控制面双义**：
  - **(i) 系统内控制指令流**（模块 A 控制模块 B）= **值流子类**：`Choice`（路径选择）、
    `Opt`（使能门）、`Slot` 换装（实现切换）、指令值写 `State`（配置控制方向，
    v0 `ConfigCell` 已归档——方向真实）——概念 1 不新增；
  - **(ii) 运维控制面**（暂停边 / 切换 Carrier / 注入故障）= 对拓扑与载体的运行时
    操作：边门（`Opt`）、实现换装（`Slot` `swap`/`swap_and_drain`，C5）、故障注入
    载体（测试双——同为"控制编码为值"，行使者 = 系统外运维者）。
- **共形命题**：控制与观测都是**值流 / 物理接口的形态**，不为它们新增概念（§8.3）；
  唯一缺口在物理层接口标准化与遥测规范（与 §7 接缝契约同源；台账 B1/B4）。

---

## 9. 后续构建议程（2026-08 规划）/ Follow-on Agenda

承接 M2 剩余与外部审计采纳（台账 C14/C15），按序推进：

1. **M2 收口**：C7 第二层（背压载体注入、EX 泛型、SlotDrive 协同）＋ 激活义务补丁
   （C15-1：授权 ≠ 取得）。
2. **A 类（工程可用门槛）**：A1 背压饱和策略枚举（`Block/DropNewest/DropOldest/Fail`
   进载体声明 ＋ `validate_seam`）；A2 bench 阈值 CI（E1 自动化）；A3 catch_unwind
   边界载体 ＋ "cell 内禁 panic"约定；A4 枚举式候选集 Slot；A5 稳定性/版本政策。
   — **A1 已落地（2026-08, 667ee92）**：饱和枚举与偏序
   （`meets_saturation_floor`）＋ `validate_saturation`；门折进**剖面装配**而非
   `validate_seam`（饱和下限是部署剖面属性）。**A3 已落地**：`drive_catch`（catch_unwind）
   既存于 flow.rs，补 `NoPanic` ④声明标记纪律。
3. **理论补丁（C15）**：激活义务小节（foundations）、错误代数小节（§9.2：E 传播/
   合并政策成文——类型层已强制 E ∈ Out，政策层自由）。
4. **B 类（说服力）**：B1 标准遥测接口（观测面落位）；B2 多 crate 分层示例；B3 中等
   真实用例；B4 控制/观测共形示例。
5. **C 类（中长期）**：会话协议端口（接 C3 残留）、效果标注系统（义务四轴需求侧，
   fail-closed ＋ 可推断）、密封白名单注册表（不封 Carrier——生态开放 §7）、拓扑级
   资源预算可行子集（线程可数、分配可求和、栈深不可判）。


## 10. 收敛纪律与自指条款（2026-08 处置清单落地）/ Convergence Discipline and Self-Reference

### 10.1 小检查器条款（Small-Checker Clause）

**纪律**：证据机械（docgate、账本探针、lint）的新增与既有修订，须满足：
- **单文件**：每机械限一文件（可人审尺度）；
- **单一断言形态**：探针为一条布尔断言（或等价极小形态），不做多断言脚手架；
- **人审＋版本历史**：审计审计器 = 人工审查 ＋ git 版本历史记录，**不建第三层
  机器自检**——递归自检违反 M9 终止条件（向上无限）；真理终点是人审与历史。

理由：de Bruijn 判据要求检查器小到可人审；axiom 的证据机械按模块数增长时，
该条款把"在未设门槛的情况下变大"变为显式门槛。违例 = 机械不符合条款但已合入。

### 10.2 服务放置可判定性表（Service-Placement Decidability Table）

**Z1 的 placement 变量展开**（`edge_cost = f(carrier, placement, types)` 中
placement 的语义面）：服务的跨机器放置按可判性分四层，各层以不同机制承接：

| 判定层 | 内容 | 机制 | 模态 |
|---|---|---|---|
| 契约相合 | 型位 ↔ 契约（消息/方向/义务） | `Conforms`/装配校验 | ②③ |
| 投递态 | Full/Closed、断连、超时判定 | `Delivery`/`SeamPoller` | ③（异步域升模态，C6） |
| 分区行为 | 网络分区下的语义（界与监测） | 界值 + 监测（经验-D） | 经验-D |
| 共识边界 | 分布式一致性/共识 | **域外**：接缝声明确认，不承诺 | ④ |

落位：挂 **ServiceProfile 扩展行**（剖面文档附加"放置语义"栏：主战场 = 各层
义务的显式声明，而非默认未声明）。判定纪律：不得把经验-D/④ 层写成 ②③。

### 10.3 自指目的条款（Self-Referential Purpose Clause）

命题 2.7 对 axiom 自身的自反应用：

> **目的条款**：axiom 的目的 = 使复杂系统设计可审计。
> **退化态**：{伪验证（把④写成①②③）、静默假定（未展出基底）、组合不封闭}＝
> **使审计不可信**。

对应机械已部分就位：账本三层测试（存在见证/探针符号可执行/落位不降级）即
"使审计不可信"三个退化态的机械拒绝面；**缺的只是本条成文**——半页，已补。

---

## 11. 实例层形态（第三构件）/ Instance-Layer Form

> 定义（三构件）：core（`cell_core`）冻结；semantics（载体 + 接缝族，零外部依赖）；
> instances（实例层，经 socket 以 feature 门控接入可替换实现）。落地：
> workspace 收敛 + EX 泛型化 + instances crate（见 instance-layer-design.md——本地未跟踪
> 工作稿，新 clone 挂空；权威内容已并入本卷 §11）。

**命题（实例可替换域）**：「可替换」谓词的域 = 机械实现（值怎么流动 / 怎么等 /
怎么观测）；权威与协议（③ 验证器、LEDGER 账本行、契约判定）不可替换——替换则权威悬空。

- **workspace 单向**：`axiom ← axiom-semantics ← axiom-instances`；成员表 + 依赖图强制，反向依赖拒绝（M6 不可组合）。
- **feature 门控默认全关**：官方实例融合单 crate（统一版本）；第三方自建独立 crate
  （双形态边界：版本粒度解耦面归开放路径，非静默合并）。
- **socket 纪律**：仅在第二实现者或真实需求出现时开设（discipline §6.4）；
  现有三位：`Executor` / `Carrier` / `Telemetry`。
- **双剖面**：门剖面（Kernel/Service，`Registered` 密封白名单，未注册编译失败）与
  开放剖面（Tool/Embedded，未注册放行）——「未注册不可选型」仅门剖面成立。
- **EX 泛型化 additive**：`poll_with`/`roll_with` 把等待点交给 `&mut impl Executor`
  （`ThreadExec` 语义不变，现有入口保留）。
- **真异步驱动（§5.4 已落）**：同步 `park` 内 `block_on(tokio::time::sleep)` 实测三形态均
  no-reactor（接入失败）⟹ 真接入改走 **adapter 侧 async worker**（`axiom-instances/async_driver`）：
  `poll()`/`roll()` 公开入口不变，等待点经 `tokio::time::sleep(tick).await` 兑现——
  不扩 `Executor` 契约、零 runtime 改动，非破坏、无 §4.3 许可。`TimedOut` 由真定时器
  产出（Timeout 升 ③ 的机制地面，D2 承载域）；账本行升 ②③ 属 LEDGER 权威变更，后续做。
- **开放项（不宣称②③）**：同步 `Executor` 插座自身仍为占位（`park_timeout`，不提供
  tokio 期限）——trait 化的可替换等待点由 `ThreadExec`/`TokioExec` 兑现，真 tokio 语义
  走 async 路径；MSRV/行为待实测。

**层位学说（承接前置"层位身份"段）**：
- **抽象层的纯承诺**：core 与 semantics 以 `no_std` 交付，是可移植的纯承诺（前置段"no_std = 抽象层纯承诺"），
  不当作终点或特权。
- **std 可替换、无特权**：实例层支持任意绑定世界（`std` / tokio / io_uring……）；不把 `std` 或其类型立为接缝
  契约——**接缝按能力声明**（`Executor` 可替换等待点 / `Carrier` 可替换传输 / `Telemetry` 可替换观测），不按
  `std` 具体类型。唯一不可替换的底 = 语言核心 + `alloc`。
- **绑定爆发归此层**：`no_std` 在抽象层保持，绑定与宿主依赖在实例层爆发；突破 `no_std` 是实例层的合法行为，非缺
  陷。
- **基座优先落地**：同步纯 cell 分布性是基座（并发 / 异步）的退化极限，见 D10 边界；实例层提供真异步驱动
  （tokio adapter worker，见上），使基座优先在绑定层可兑现，而非把异步当作愧对 no_std 的附加。
