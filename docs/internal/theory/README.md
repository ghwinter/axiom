# theory 目录索引（axiom 内部理论工作区）

# Theory Directory Index (axiom Internal Theory Workspace)

> **性质**：I1 层理论工作区索引（`docs/internal/theory/`）。本目录是 axiom 的理论
> 研究语料：两份规范论文（现行、审定）、一份现行前沿注记、一份历史归档。
> 规范性表述一律以公开文档 `docs/en-us|zh-cn/` 为准；本目录不承载承诺。

---

## 1. 文件角色

| 文件 | 角色 | 状态 |
|---|---|---|
| `boundary-ontology.md` | 规范论文——代数合法性与机器可达性的四轴结构，及双层信任架构（闭包系统/合法重接线/三归宿/分层律） | 现行规范（审定） |
| `meta-foundations.md` | 规范论文——公理放置问题 M（构成三分区/回归三难/证明分层/诚实规则/义务代数） | 现行规范（审定） |
| `frontier-notes.md` | 现行前沿注记——六条未落地方向（高阶绑定/时间作值/封闭极小规范 API/产物分析粒度阶梯/e-graph）+ 诚实边界 | 现行（不构成承诺） |
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
  本目录 4 个文件均进入跟踪。
- **范围说明**：顶层仓库 `D:\Projects\ICodeV7` 是独立 git 仓库，其 `.gitignore` 的
  全忽略规则不适用于本仓库；无需在其内添加白名单。
- 本目录 4 个文件进入跟踪后：规范论文与归档不得删除；`frontier-notes.md` 的修改须
  保持第 3 节文体规则。