# sqlmini — SQL 子集编译器 + 执行引擎

> **尺度（双参照，诚实标注）**：绝对尺度为**微型 SQL 子集引擎**（对标 SQLite/
> PostgreSQL 属演示级：约 2200 行、单表、无优化器）；仓库相对尺度为 **axiom 最
> 大单个示例**（超过 redis_like/netpath/psql 等）。用途 = 范式证据（验证 axiom
> 能否承载多阶段真实语义管线），**不声明**引擎工程级规模或性能。

## 系统

单表 SQL 子集：`SELECT [DISTINCT] 表达式列表 FROM 表 [WHERE 谓词]
[GROUP BY 表达式] [ORDER BY 表达式 [ASC|DESC]] [LIMIT n]`，聚合
`COUNT/SUM/AVG/MIN/MAX`；数据源为 CSV；输出文本表。

规模（`runtime/examples/sqlmini/`）：约 2200 行（lexer 290 / parser 490 /
planner 330 / exec 620 / ast 190 / data 110 / schema 60 / main 150），
含 38 项测试。

## 架构（axiom 词汇）

```
文本 ──▶ [Lexer] ─▶ [Parser] ─▶ [Planner] ─▶ 计划 ─▶ [执行器] ─▶ 结果表
        cell          cell          cell              Inline / 分区并行
        └────────── TryChain<TryChain<Lexer, Parser>, Planner> ─────────┘
```

- 前三个阶段各是一个 `PortCell`（后者 `In` = 前者的 `Ok` 值）；整链 =
  `TryChain` 嵌套，**失败为值**（`SqlError{Lex,Parse,Plan,Exec}`，带位置/对象），
  短路经类型保证（`Err` 时下游不执行）；T1 对偶由 `Conforms` 判定。
- Planner 的 `State = Schema`：驱动前注册，未注册拒绝（不猜测列）。
- 执行器两条物理路径共用同一计划：**Inline**（单线程逐行）与**分区并行**
  （Filter+组累积分线程；聚合在「合并」下可结合——COUNT/SUM/MIN/MAX/Avg 可分；
  ORDER/LIMIT 不可分，合并后统一执行）。两者结果逐行相等（**T6**）。

## 范式验证点（axiom 主张 ↔ 本例证据）

| axiom 主张 | sqlmini 证据 |
|---|---|
| 五概念可承载真实软件 | 编译管线 4 阶段 + 执行器 = 因果流组合，无新增概念 |
| 失败为值、短路、typed errors | 全链单 E；任意阶段错误带定位，测试锁定 |
| 多物理等价（T6）可验证 | 双路径逐行相等（测试覆盖 4 类查询） |
| 义务/预算纪律 | 执行错误为值（Exec 变体）；无 panic 约定 |
| 组合封闭 | `TryChain` 嵌套 + `Schema` 状态注入，均过 §8.3 形态 |

## 运行与测试

```
cargo run   --manifest-path semantics/Cargo.toml --example sqlmini -- "SELECT …"
cargo test  --manifest-path semantics/Cargo.toml --example sqlmini
```

## 已知子集边界（诚实声明）

- 单表；无 JOIN/嵌套查询/事务；CSV 无引号/转义（字段 = 逗号切分）；
- `ORDER BY` 键仅支持输出列名（表达式键落 `NULL`）；
- NULL 语义简化三值：谓词遇 Null 为假、聚合忽略 Null、运算传播 Null
  （`COUNT(*)` 计行数、其余聚合数数值）；
- 无索引/优化器（Planner 只做合法性与基本类型检查——优化是另一工程量，
  不在本例承诺内）。