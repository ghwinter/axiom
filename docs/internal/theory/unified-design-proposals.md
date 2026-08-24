# 统一模型：核心设计提案与推理记录（内部理论）

> **状态（2026-08-23 更新）：本提案已全部落地，本文降级为推理记录。**
> ②型位（`Slot<I,O>`+`Conforms`）、③schema 构造子（`Rep`/`Repeat`、`Choice`/`Opt`）
> 均已在 core/runtime 实现并有测试；**"代换绑定三态统一"已收敛定稿**——
> `Slot`（未绑/定义）/`Wire`（编译期 ∀ 绑定）/`SlotDrive`（运行期 ∃ 绑定）是
> 同一次代换的三个绑定态，规范性表述见 `docs/{zh-cn,en-us}/unified.md` §2.3。
> 文中各处"仍待"标注均已被下文 B 第一步～第三步的完成事实取代；
> 权威口径以正式集为准，本文不再维护。

> **性质**：I1 层设计提案 + 推理记录（`docs/internal/theory/`，不入 git）。
> **关系**：正式规范层已有 `docs/zh-cn/unified.md` 与 `docs/en-us/unified.md`（统一模型
> 的规范性表述）。本文件记录**尚未落地、属探索/路线**的部分：②型位与③递归 schema 构造子
> 的**核心设计提案**，以及把 axiom 从"静态蓝图实现"推向"统一设计实现"的路线与取舍。
> 本文件不进入正式集——因为其中的构造子**尚未实现**，不是已成立的事实。

---

## 1. 本轮推理链（为何视角升级）

多轮讨论收敛出的关键洞察（均已进入正式 `unified.md`，此处只记结论与动机）：

1. **统一视角 = 类型化接口代数上的代换演算**：软件 = 带型位的图样；运行/构建 = 把每个型位
   代换为符合接口的居留项；静态 = 编译期全称绑定（∀），动态（插件/装载/驱动热插拔）= 运行期
   存在实例化（∃）。二者是**同一个代换操作的两种绑定模态**。（正式对应：操作类/对称幺半范畴、
   多项式函子/容器、依值类型论。）
2. **定义 ↔ 激活 轴**（正交于静态/动态）：定义 = 良定义已验证的结构（类型平面、零成本）；
   激活 = 嵌入一次运行（值沿因果边流动）。二者独立——可定义而永不激活（`if false` → 运行 0）。
   axiom 核心是**定义（潜在）的代数**；激活是运行/载体（实然）侧。
3. **三种代换形式**：①静态组合（core 已有）②型位（∃，运行期代换一个居留项）③递归/生成
   schema（有限封闭 schema F 的不动点 → 无限实例网）。型位限**种类**、递归限**数量**。
4. **"动态修改"的墙**（正式 §5.9）：运行时只能替换**封闭接口内**的占用/内容，不能改接口/形状
   ——正因为接口/ABI/协议固定，动态才可能（T1+T5+A2/B2）。
5. **schema 表达力阶梯**：有限 → 正则/星（Kleene，自由单子，"类正则"）→ 代数（互递归）→ 一般图
   （不可判定）。未来合规用 (余)归纳/可派生性证明 = **逻辑封闭而非实例封闭**。
6. **动态税精确化**（正式 foundations §3.3）：5 项分解；是物理边界机制的函数，axiom 中立且局部化。

---

## 2. 核心设计提案（未实现）

目标：让 axiom 的核心（`cell_core`）从"只做①静态组合"进化为"①+②+③"，成为统一设计的实现。
所有构造子**仍是"定义"**（零大小、编译期定型、T1 可证），激活仍由 `drive`/运行/载体分离承担。

### 提案 ②：型位（Loadable Slot / `Slot<I,O>`）

- 语义：接口固定、密封（sealed）；编译期对封闭接口族做**参数化 T1 验证**
  `∀ T: Interface, wiring(SRC → T) 合法`；运行期存在实例化一个居留项（代换）。
- 组成：`Slot<I,O>`（型位：`In=I, Out=O`，接口密封）在类型平面声明；居留项以 trait bound/
  ABI 在注册缝校验。
- 意义：把"动态"从**物理例外缝**（当前用 Result/FFI 拼凑）提为**核心内与静态同构的代换**。
- 悬而未决：型位居留项的传递/所有权、跨线程（T9 物理）、失败通路（`Result` 约定，正式 §9.2）。

### 提案 ③：schema / 文法构造子（生成器）

- 语义：从**有限封闭 schema** 生成**无限实例网**；证明在编译期对 schema 的性质 = 对代换的
  (余)归纳。
- 组成（正则/Kleene 层，优先）：`Choice<A,B>`（或）、`Opt<C>`（可选）、`Star<C>`/`Repeat<C>`
  （任意次，自由单子）、`Plus<C>`（1+）。可选：代数层互递归（节点种类相互引用）。
- 意义：表达"按协议连接任意多个设备形成树"——数量无限来自递归，种类仍封闭。
- 悬而未决：证明机制（初始代数结构归纳 vs 余代数 guarded 互模拟）、单态化体积上限（正式 §7 开放
  问题 2）、star/递归可展开的编译期代价。

### 提案 ①′ ：激活分离（已隐含，无需新增构造）

- "定义可能不激活"已是 axiom 现有的性质（蓝图零大小、`drive` 才产生运行）。提案只要求文档化
  并让 ②③ 同样"激活分离"，不引入隐式自动激活。

---

## 3. 路线与取舍

| 步骤 | 内容 | 验收 |
|---|---|---|
| S1 | 在 `cell_core` 加 `Choice`/`Opt`/`Star`/`Repeat`（Kleene 层） | 编译、T1 对每次重复合法、测试 |
| S2 | 加 `Slot<I,O>` 型位（密封接口 + 参数化 T1） | 编译期 `∀T` 验证、运行期代换、测试 |
| S3 | （可选）代数层互递归 schema | 编译、测试 |
| S4 | runtime 配合：型位居留项的物理载体（装载=物理，T9） | 跨载体语义等价验收 |

**封顶决策**：compile-time 可证 schema 类封顶在 **正则/代数**；一般动态图（任意共享/环/拓扑改写）
不可判定，归物理/验证边界（正式 T9 显式例外），不做编译期可证承诺。

**诚实边界**：
- 这些都是**设计提案**，未实现、未承诺；不在正式规范中展示为已成立事实。
- 行为等价（A5）仍是最难项，未实现（正式降级声明不变）。
- ②的失败通路、③的单态化体积，均悬而未决，须在实现时再收敛。

---

## 3b. 实现进展（已落地，验证全绿）

- **P1（③ 有界星）**：core `Rep<N,C>`（正则/星，`RepState` 手动 `Default`、`N=0` 恒等、
  编译期 T1 验证、4 测试通过）。
- **P2（② 定义侧）**：core `Slot<I,O>` + `Conforms`/`assert_conforms`（编译期参数化 T1，
  "未来任何 `In=I,Out=O` 的居留项合规"，1 测试通过）。
- **P3（激活侧，runtime/std）**：`SlotDrive<I,O>`（∃ 存在化填充：install/swap/drive，类型擦除
  为 `Box<dyn Any + Send>`）+ `drive_seq`（无界计数序列驱动），3 测试通过。
- **A（核心正则算子补全）**：一等纯 `PortCell` 的 `Choice<A,B>`（输入标号并，纯确定）+ `Opt<C>`
  （可选，`Option` 变换），prelude 导出，3 测试通过；闭合"正则 `|`、`?`"。
- **B（runtime 错误/短路通路）**：`drive_try<A,B,X,E>`（`Out=Result` 约定 + 短路，no_std 安全），
  1 测试通过；闭合 §9.2 的短路侧。**设计改进（D）**：新增 `TryChain<A,B>`（两个会失败的 cell
  的单层 `Result` 短链 PortCell），解决 `drive_try` 在 `B::Out=Result` 时嵌套的冗余；psql 以
  `TryChain<TryChain<Lexer,Parser>,Executor>` 表达整条 REPL，三层错误合一短路。
- **B（有界/背压）**：`BoundedQueue<T,CAP>`（buffer.rs，std，基于 sync_channel；`push` 阻塞=
  背压、`try_push` 满返回 `Err`=容量信号），2 测试通过；闭合 §9.1 的原语侧（接成 `Carrier`
  仍是开放项）。
- **R（有界/背压载体，§9.1 闭环）**：`BoundedCarrier<CAP>`（有界通道形态的 `Carrier`）+ 
  `bounded_pump<A,B,It,CAP>`（**真实阻塞背压**：生产端满时阻塞、消费者线程 drain；返回输出
  序列），backpressure 2 测试通过。
- **C（psql 健壮性，驱动 runtime）**：把 psql 改造为**会失败**的 REPL——`Lexer`/`Parser` 的
  `Out = Result<_, PErr>`（词法/语法错显露而非静默吞掉），`Executor` 报执行错（表不存在）；
  主流程用 `drive_try` 对 Lexer→Parser 做短路（语法错不流入 Executor）；顺带修复 `SELECT *`
  未识别的真实 bug。同时清掉 mmo/redis_like 的既有 `let_unit_value`警告 → runtime **全部
  目标（lib+examples+benches）clippy 零警告**。
- **C（redis_like 健壮性）**：加 `Config` 资源边界（max_keys/max_value，超限拒绝）、
  `Cmd::Protocol`（缺参/非法值不再静默成 0/空 → RESP `-ERR`）、`Reply::Err` 编码；主演示含
  值过大、键数超限、未知命令。4 例（psql/redis_like/mmo/netpath）全部 clippy 零警告并运行通过。
- 全程 core 零依赖、no_std、`#![forbid(unsafe_code)]`；core 15 测试 + runtime 10 测试全绿，
  lib clippy 干净。示例（mmo 等）有**先存**的 clippy 警告，属用例工程，不在本目标范围。

**未实现（仍为提案）**：无（本目标 R/K/T 已闭合）。**已裁定/已落地说明**：
- **R（有界/背压）已闭环**：`BoundedQueue` + `BoundedCarrier` + `bounded_pump` + 可失败
  `bounded_pump_try`（失败×背压联合语义）。
- **K（代数/递归 schema）已裁定**：**无需新核心组合子**——递归/互递归图样由用户递归
  `PortCell` + 既有组合子（`Rep`/`Chain`/`Choice`/`Opt`）表达（测试 `recursive_cell_type_composes_with_t1`）；
  无界生成性展开归 ∃/物理侧（`drive_seq`/有界泵）。
- **T（失败/全函数）已裁定**：**不公理化**——`step` 保持全函数，"失败"是 `Out=Result` 的
  值；穿过组合由 `TryChain`/`drive_try`（短路）承担（正式 foundations §7 开放问题 5）。

**定稿与重构（目标）：**
- **A（theory 定稿）**：`foundations.md` §8"封闭的核心边界"（中英）——五个不可约构造概念
  （Cell/T1 对偶组合/组合封闭/代换绑定/激活），其余皆实例、无第 6 概念；封闭性判定标准；
  构造概念 vs 性质公理 vs 运行策略（调度/并发是激活的外部策略，axiom 不立法）；§8.6 深化
  澄清（step=Moore 非运行时状态机；连接=类型化因果流非函数；非阻塞原子 step 纪律；动态税=
  推迟选择非创造；运行期绑定=选择+激活）。
- **B（代码收敛，第一步）**：**T1 判定统一为一个**——`DoesWire` 与 `Conforms` 合并为统一
  `Conforms<EXPECT>`（`Wire<A,B>`=布线、`Slot<I,O>`=装载，两类"类型化位置"），`assert_wiring`
  经 `assert_conforms`；`DoesWire` 全库移除（core/runtime/macros/tests/examples/文档同步）。
  仍待：把"代换绑定"收敛为一个原语（Link/Slot/SlotDrive 三态）、命名收敛（Chain/CellChain 二义）。
- **B（第二步，命名收敛）**：`CellChain` 改名/收敛为单一 `Chain`（去掉别名 `type Chain = CellChain`），
  核心/运行时/benches/examples/文档（中英）全库同步，无同义不同名残留（`CellChain` grep 归零）。
  仍待：把"代换绑定"收敛为一个原语（Link/Slot/SlotDrive 三态统一；静态=编译期绑定、
  动态=运行期绑定）——这是统一代换在代码层的关键一步。
- **B（第三步，代换绑定命名收敛）**：`Link` 合并进 `Wire<A,B>`（它同时是"类型化位置/接口"
  与"编译期绑定组合动作 `fire`"）——消除我此前引入的 `Link`/`Wire` 同义不同名；绑定族现为
  统一命名：`Slot`（未绑/定义）、`Wire`（编译期绑定）、`SlotDrive`（运行期绑定），T1 统一
  `Conforms<EXPECT>`。跨 crate 的**单一** `Bind` 类型按 §8.4 分层被有意保留在各自层
  （核心=定义、runtime=激活），不做字面合并。

**审计修复（goal-a27ebec4，runtime 代码）**：S1 `spawned_flow` worker panic 经
catch_unwind → 回执通道 → 调用方 resume_unwind 传播（终止性修复，测试覆盖）；
S2 `BoundedCarrier` flow 内 `const { assert_capacity_nonzero::<CAP>() }` 编译期门（拒绝
CAP=0 死锁态）；S3 `BoundedQueue::push` 断连返回 `Err(值)`（不静默丢）、`pop`/`try_pop`
用 `Result` 区分空/断连、`spare`→`capacity`；S4 删除死类型 `ChannelCarrier`；
S5 删除 `drive_wired` 假 LINK 见证（≡drive_link）；S6 删除 `DirectCarrier`≡`InlineCarrier`
副本（`static_path`/lib/示例同步）；S7 门禁落地：新增 `drive_feedback_inline`（要求
`FEED: Moore`）+ 测试；S8 去掉 `drive_seq` 多余 std 门控（no_std+alloc 可用）；S10
runtime Cargo 依赖 `axiom` `default-features=false`、`std=["axiom/std"]`（no_std 组合闭环）；
S12 `CarrierCost` 默认改 `External`（保守，防忘写自诩零分配）；S14 TryChain 措辞修正。
核心的 `Rep` 互 From 双界（C1）与 `Feedback` 单元双拍（C2）**留待用户另议**，未动。

## 4. 溯源

- 正式规范：`docs/zh-cn/unified.md`、`docs/en-us/unified.md`（统一模型）；
  `docs/zh-cn/foundations.md` §3.3（动态税）、§5.9（型位/墙）、§7（开放问题）。
- 本提案服务于：把 axiom 从"静态蓝图实现"推向"统一设计实现"；若落地，将同步更新正式文档。
