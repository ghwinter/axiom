# doc-governance.md — 文档标准与决策记录制度

> **性质**：axiom 的文档工程标准（M1）与决策记录制度（M2）。本文档定义
> 文档分层（tier taxonomy）、字数预算、写作纪律、以及"为什么这样设计"
> 的记录规范。目标是让文档**每事实只有一个家（one home per fact）**、
> 可机械检查、不膨胀。

---

## 一、文档层级（tier taxonomy）

**原则**：每条事实有且仅有一个"家"；其他层只链接、不复述。

| Tier | 职责（只放这里） | 文档 |
|---|---|---|
| 根 README | 项目定位、快速上手、能力总览、测试状态 | `README.md`（英文）· `README.zh.md`（中文） |
| 哲学 | 世界观：抽象/物理解耦、零成本、静态优先 | `docs/philosophy.md` |
| 基础 | 代数基础：公理-定理-证明，形式化映射 | `docs/foundations.md` |
| 架构 | 架构细节：端口、链接、部署、runtime、组合子 | `docs/architecture.md` |
| 原则 | 元问题与设计原则（零成本范式、验证判据） | `docs/design-principles.md` |
| 适配器 | adapter 生态规则、runtime contract 认证门槛 | `docs/adapters.md` |
| 图 | 系统分层、载体矩阵、路线图 | `docs/architecture_diagrams.md` |
| 文档标准 | 本文档：tier、预算、纪律、决策记录 | `docs/doc-governance.md` |

**双语惯例**：文档系统只使用中英两种语言。根 README 采用双文件——
`README.md`（纯英文）+ `README.zh.md`（纯中文），顶部互相链接
（`English | [中文](README.zh.md)`）。docs 其余文档以中文为主（术语保留
英文原文）；若某文档需要英文版，沿用 `docs/<name>.zh.md` 对映命名。

**one-home 规则**：同一事实（如"静态路径是串并联 DAG"）只在一个 tier 详述，
其他层链向它。新增内容前先定位"它属于哪个 tier"；若多个层都需要，只有一层
详述，其余链接。

## 二、字数预算

**单位**：按空白分割的粗略词数（中文文档此统计低估，仅作趋势门禁）。

| 文档 | 当前 | 预算上限 |
|---|---|---|
| `README.md` | — | 2500 |
| `docs/philosophy.md` | 4107 | 5000 |
| `docs/foundations.md` | 3411 | 4000 |
| `docs/architecture.md` | 5476 | 6500 |
| `docs/design-principles.md` | 494 | 1500 |
| `docs/adapters.md` | — | 1500 |
| `docs/architecture_diagrams.md` | 1692 | 2000 |
| `docs/doc-governance.md`（本文档） | — | 1500 |

**超限处理**（按序）：① 把属于其他 tier 的内容迁移过去，留一行链接；
② 压缩本层表述；③ 仅在内容确实需要时提高预算并记录理由。
**门禁命令**（PowerShell）：

```powershell
Get-ChildItem docs\*.md | ForEach-Object {
  $w = (Get-Content $_.FullName -Raw) -split '\s+' | Where-Object { $_ -ne '' }
  "$($_.Name): $($w.Count) words"
}
```

## 三、slop 清单（写作时逐项自查）

- **叙述历史**："previously / now / 已改为 / 曾"——写当前事实，变更故事进
  提交信息与决策记录。
- **状态标注**："implemented! / future: …"——状态会腐化，仓库与代码是
  权威，文档写当前现实。
- **手抄目录/清单**：凡可从源码生成的（catalog、矩阵），不手写。
- **推理转录**：不要"为什么这么做"的长篇推导过程，保留结论与一句理由，
  详述进决策记录。
- **段落墙**：一段多规则、多括号插入语——拆段或降级到归属层。
- **强调通胀**：全篇加粗/CAPS 等于无强调——只强调改变行为的子句。
- **隐喻泛化**：用精确术语（"契约"只用于义务/不变量，"边界"只用于字面的
  流程/安全/事务边界），不滥用。

## 四、决策记录制度（M2）

**目的**：axiom 是哲学驱动项目——"为什么"与代码同等重要，且决策理由应在
仓库中可回看，不散落在对话/讨论里。

**三态结构**（`docs/decisions/`）：

```text
docs/decisions/
  proposed/     提案中的决策（待实施/待评审）
  implemented/  已实施并验证的决策（现在时描述）
  archived/     已冻结的历史记录（不再修改）
```

**记录模板**（每个决策一个文件）：

```markdown
# <决策名>

- **状态**：proposed | implemented | archived
- **日期**：YYYY-MM-DD
- **动机**：为什么需要这个决策（元问题/约束/实证）
- **决策**：结论（一两句）
- **理由**：为什么是这个选择（可多句）
- **代价**：放弃了什么 / 边界是什么
- **验证**：如何验证（测试/bench/断言）
```

**规则**：
- 非平凡变更（新契约、执行模型变更、API 破坏）必须伴随决策记录；
- implemented 用现在时描述已发货现实，不用 "should / 将";
- archived 冻结，永不修改（新情况开新记录并链接旧记录）。

**现有决策索引**（历史记录在 `docs/design-principles.md` 附录与迭代记录中）：
D1 物理 = 有限执行形态集合 · D2 性能差距先分类 · D3 显式 > 隐式 ·
D4 单一事实源 · D5 验证判据 · D6 来源/去向是业务错误 · D7 执行形态同构。
新决策沿用此编号（D8 起）并落盘到 `docs/decisions/implemented/`。

## 五、写作规则

- **当前状态**：写系统现在是什么样，不写它是怎么变的。
- **链接优先**：跨 tier 引用用相对 Markdown 链接，不裸写文件名。
- **代码/标识符保持原文**：类型、trait、函数名、路径用原样（不翻译）。
- **中英双语**：中文文档为主，术语保留英文原文（`Machine`/`FlowKind` 等）。
- **提交配套**：改行为（契约/语义）的变更，同一提交内更新相关文档 +
  决策记录。

---

> **一句话**：文档的分层、预算、纪律与决策记录共同保证 axiom 的"为什么"
> 可回看、可检查、不膨胀——这是 P3 工程化的地基：代码的验证力（P0–P2）
> 与文档/决策的可审计性（P3）共同构成 axiom 的"可验证 + 可追溯"。
