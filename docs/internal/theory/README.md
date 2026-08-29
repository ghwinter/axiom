# theory 目录索引（axiom 内部理论工作区）

# Theory Directory Index (axiom Internal Theory Workspace)

> **性质**：I1 层理论工作区索引（`docs/internal/theory/`）。本目录是 axiom 的理论
> 研究语料：两份规范论文、一份规范设计、一份理论注记、一份现行前沿注记、一份定位注记、一份历史归档。
> 规范性表述一律以公开文档 `docs/en-us|zh-cn/` 为准；本目录不承载承诺。

---

## 1. 文件角色

| 文件 | 角色 | 状态 |
|---|---|---|
| `boundary-ontology.md` | 规范论文——代数合法性与机器可达性的四轴结构，及双层信任架构（闭包系统/合法重接线/三归宿/分层律） | 现行规范（审定） |
| `meta-foundations.md` | 规范论文——公理放置问题 M（构成三分区/回归三难/证明分层/诚实规则/义务代数） | 现行规范（审定） |
| `runtime-constitution.md` | 规范设计——runtime 宪法的落地蓝图（公理 A1–A6/定义 D1–D5/义务代数/代码投影/破坏性变更清单/兼容性与开放接缝） | 现行规范（Δ 执行中） |
| `frontier-notes.md` | 现行前沿注记——七条未落地方向（高阶绑定/时间作值/封闭极小规范 API/产物分析粒度阶梯/e-graph/蓝图编译期载体形态/效果标注系统）+ 诚实边界 | 现行（不构成承诺） |
| `positioning.md` | 定位注记——六个科学透镜（原子组合/系统论/信息论/涌现论/计算科学/现代数学）对既有构件的解读；两处锚点增量（迹单调范畴＝Feedback 形态、模态的双重逻辑读法） | 现行（非新公理；冲突以规范为准） |
| `incompleteness-unification.md` | 理论注记——不完备统一：Gödel–Rosser 不可完备化 / Tarski 真不可定义 / Lawvere–Yanofsky 不动点定理的同一骨架与层级差异；axiom 边界词汇对应表；"隧道式分层"设计推论（自指⇒不完备⇒层间双向残差必然） | 现行注记（非新公理；冲突以规范为准） |
| `theory-archive.md` | 历史归档——六份早期文档的唯一内容合并（推导档案/元层推理/落地审计/原则重审/执行日志） | 归档（不再演进） |

## 2. 超驰映射（supersession map）

六份早期文档已合并入 `theory-archive.md` 后删除；其重复内容由下列公开文档承接：

| 已合并来源文档 | 重复内容的承接方 |
|---|---|
| axiom-theory-foundations.md | docs/en-us|zh-cn/foundations.md §0–§7（承诺/理论家族/公理集/T1–T9/数学表达/蓝图形态/边界/开放问题）；docs/en-us/core.md §5（理论 ↔ Rust 对应） |
| paradigm-notes.md | foundations.md §1.0（三态定义：本体/退化/构造）、§5.8（FlowKind 注解）、§8.2–8.4（无第六概念/封闭判据/分层）；boundary-ontology.md §6–§7；meta-foundations.md 定义 1.5 |
| unified-design-proposals.md | unified.md §2.3（三绑定态 Slot/Wire/SlotDrive）、§6；core.md §6b；runtime.md §3b/§9 |
| compile-time-core-direction.md | core.md §1–§6、§8；foundations.md §5.3/§5.5 |
| refactor-plan-runtime-carriers.md | runtime.md §1–§6 |
| refactor-plan-compile-time-core.md | core.md §2–§6 |

## 3. 文体检查表（核心规则）

完整清单见重组分析报告的检查表 C；本目录强制执行下列规则：

1. **无第一人称**：我们/我/咱们一律删除或改写为"本文/本记录/当时决策"。
2. **无情绪词**：胜利/完美/危险/背叛/更糟/吸引力/误诊等换为结论/印证/风险/误导/
   潜在价值。
3. **无 AI 味填充**：全绿/闭环/顺带/一句话/大胆/值得注意等换为全部通过/完成/并/概括/
   受控收敛/需注意。
4. **无语问句与修辞性标题**；小标题使用 定义/命题/定理/推论/观察/结论/边界。
5. **术语统一**（与公开文档一致）：型位（typed hole）、布线/连接实例、三绑定态
   （`Slot` 未绑/定义、`Wire` 编译期绑定、`SlotDrive` 运行期绑定）、四模态 ①②③④。
6. **历史内容一律附承接标注**：`superseded by: docs/<en-us|zh-cn>/<doc>.md §<号>`；
   标注不确定者写"见对应公开文档"。

## 4. git 跟踪说明

本目录受版本控制（它是唯一保留早期执行记录与决策理由的载体）。状态：

- **白名单已生效**：`core/axiom/.gitignore` 以 `/docs/internal/*` 忽略内部工作区、
  并以 `!/docs/internal/theory/` 显式放行本目录；在 axiom 仓库内核对
  `git -C core/axiom check-ignore docs/internal/theory/<file>` 返回 exit 1（未忽略），
  本目录 8 个文件均进入跟踪。
- **范围说明**：顶层仓库 `D:\Projects\ICodeV7` 是独立 git 仓库，其 `.gitignore` 的
  全忽略规则不适用于本仓库；无需在其内添加白名单。
- 本目录 8 个文件进入跟踪后：规范论文与归档不得删除；`frontier-notes.md` 的修改须
  保持第 3 节文体规则。

## 5. 审计记录 / Audit Record

> 性质：陈述可能性审计（statement-possibility audit）的记录。范围：本目录 8 个文件全量。
> 方法：逐文件比对近两轮讨论产出的陈述集合与文档已陈述集合，检查三类缺口——陈述缺失、
> 索引不一致、计数过时；同时检查阴性面（无缺口的方向）。

**发现并修复（5 项）**：

1. **第五轴（目的相容/admissibility）命题缺失** → 补入 `boundary-ontology.md` 命题 2.7
   （退化态定义、目的过滤器部分机械化、与四轴的独立性证明纲要）及注记 2.8（选择轴包含链）。
2. **矛盾分类学缺失**（类别矛盾被消除/边界矛盾被定位/经验矛盾被展出）→ 补入
   `boundary-ontology.md` 注记 9.10，并标注其与"无第六概念"判据适用范围的区分。
3. **库兼容性命题缺失**（接缝契约 + 义务声明 + 机制自由；三处边界；与早期声明兼容矩阵）→
   补入 `runtime-constitution.md` §7。
4. **索引漏列** `runtime-constitution.md`（已跟踪文件不在目录表）→ 本表补行。
5. **计数过时**：§4"5 个文件"实际为 7 个 → 更新。

**阴性结果（未发现遗漏的陈述可能性）**：四轴独立性（命题 2.5/注记 2.6）与第五轴的关系已由
命题 2.7 收口，无第三组轴间关系待陈述；模态格/义务代数/六元组标准化在 meta-foundations §3 与
runtime-constitution §3 已有陈述；三归宿、双层信任、分层律无新增陈述需求；`positioning.md`
六透镜解读与两处增量（迹单调范畴、模态双重逻辑读法）无缺口。

**残余开放项（已声明为开放问题，非遗漏）**：T5 行为等价、Z1 成本语义、异步载体落地、
`obligation_min()` 剖面分化——均已在 frontier-notes 或 claims 记录中标注，不属本审计范畴。
