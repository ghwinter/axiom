# runtime 宪法：义务代数的落地设计 / Runtime Constitution: The Obligation-Algebra Design

> **性质**：I1 层设计规范（`docs/internal/theory/`，入 git）。**本文是 axiom-runtime 破坏性
> 重构的蓝图**：由 [`meta-foundations.md`](meta-foundations.md)（公理/定义/代数）向代码投影。
> 上游：meta-foundations 定义 1.1–1.11、命题 7.4（六元组）、boundary-ontology 定理 9.6、§8.3 封闭判据。
> 状态：Δ 执行中；核心冻结（`src/cell_core.rs` 语义零改动）。

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
- **D5 runtime**：义务代数的机械——物理层自分层（分层律在物理层内的递归应用）。

## 3. 代数 / Algebra

- **义务组合律 f(X,Y)**：载体 C₁（类 X）⊙ 载体 C₂（类 Y）→ 复合义务类 f(X,Y)；f 的定义与健全性证明是工程项（A 若满足 X、B 若满足 Y，则 A⊙B 满足 f(X,Y)）。
- **模态格**：② < ① 不作比较；③ 依见证形态；∅ 为违例零点；每条义务恰占一格，否则构成失效（开放问题 8.3 裁定）。
- **六元组标准化 (S,L,T,C,V,R)**（meta 命题 7.4）：载体目录的发布形态——接口契约 S、规范强度 L、符合性测试 T、剖面 C、版本 V、治理 R。

## 4. 代码投影 / Code Projection（新结构）

```
runtime/src/
  contract.rs   义务账本：义务类 × 模态 × 见证 fn × 测试（A4/A5/A6 的机械）
  obligation.rs 义务类类型系统：DeliveryState / ResourceClass / ReferenceValidity / LifecyclePhase
  delivery.rs   投递四态：Full/Closed 机械化（②③），Timeout/Cancelled 声明（④，机械化为物理选择）
  slot.rs       typestate 生命周期：SlotPending → SlotLive → retired；Seat 代戳（模态①）
  mailbox.rs    有界邮箱反饥饿：容量 = buffer + 每生产者席位；三投递模式 fire/try/block
  event.rs      事件基座载体类：EventSource + pump_events（§9.3 从首案例到载体类）
  flow.rs       驱动：drive_link / assemble_link|seam / drive_seq / drive_try / TryChain / drive_feedback_inline
  buffer.rs     BoundedQueue（对齐 DeliveryState）
  carrier.rs    Carrier trait + Inline/Queue/Bounded + ResultCarrier/MaybeCarrier
  static_path.rs / macros.rs / lib.rs（prelude 按新结构导出）
```

## 5. 破坏性 API 变更清单 / Breaking-Change List

1. `SlotDrive` typestate 化：`install → SlotPending`，`commit() → SlotLive`；`retire()` 终结；未 commit 不可驱动（模态①，零运行期检查）。
2. `SlotLive` 提供 `Seat`（借用的驱动视图，携带代）；`swap` 后旧 Seat 以代校验拒绝（过期引用 = 类型/运行期可检错误）。
3. `BoundedQueue::push/try_push` 对齐 `DeliveryState`（Full/Closed 显式区分，值随错误回传）。
4. 载体目录按六元组文档化（每个载体一节：S/L/T/C/V/R）；`ResultCarrier`/`MaybeCarrier` 加入。
5. `bounded_pump` 内部换用 `mailbox`（语义不变，实现换底）或并存（教学形态保留）。

## 6. 执行阶段 / Execution Phases

1. 契约层：obligation.rs + delivery.rs + contract.rs 账本与落位律测试（A3–A6 机械）。
2. 生命周期层：slot.rs typestate + Seat 代戳。
3. 背压层：mailbox.rs + bounded_pump 换底。
4. 事件层：event.rs + redis_like 实例化。
5. 短路载体：ResultCarrier/MaybeCarrier（§9.2 收账）。
6. 示例健壮性：netpath/mmо Result-ify。
7. 终验：tests / no_std / clippy 全绿 + runtime.md en/zh 同步。

每步过 §8.3 封闭判据（无第六概念）：义务类=概念1 失败为值的展开；生命周期=概念4 型位的实现；邮箱/事件=概念5 物理载体的实例；账本=契约（§8.4 物理层义务）。