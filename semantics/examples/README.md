# semantics/examples — 示例索引

本目录是 `axiom-semantics`（契约层）的示例集合：**同一套 cell_core 蓝图 + 契约层
组合子**，覆盖不同尺度与关注点。角色分工：**概念演示**（T6 多物理等价、失败为值、
控制/观测）→ **真实语义管线**（SQL、网络、MMO）→ **组织分层**。

> 运行全部示例：`cargo run --manifest-path semantics/Cargo.toml --example <name>`。
> 凡示例自带测试，用 `cargo test --manifest-path semantics/Cargo.toml --example <name>`。

## 一览表

| 示例 | 角色 | 核心演示点 | 尺度 |
|---|---|---|---|
| [`closed.rs`](closed.rs) | 概念整合 | §8 封闭边界：组合自封闭 / 失败为值 / T6 / 运行期换装（`SlotDrive`）；no_std 核心 | 概念 |
| [`carrier_demo.rs`](carrier_demo.rs) | 载体演示 | 同一蓝图 × 多载体（Inline/Queue/Bounded）语义等价、时空成本不同（T6） | 概念 |
| [`threaded_flow.rs`](threaded_flow.rs) | 载体演示 | 同拓扑异构物理：InlineCarrier 零分配 vs `spawned_flow` 跨线程通道 | 概念 |
| [`psql/`](psql) | 真实管线 | SQL REPL：`TryChain` 单层短路，Lexer/Parser/Executor 三层错误合一为值 | 小型 |
| [`netpath/`](netpath) | 真实管线 | 网络接收路径：Eth→IP→TCP 三段解析，`ResultCarrier`/`MaybeCarrier` 双载体短路等价 | 小型 |
| [`mmo/`](mmo) | 真实管线 | 多人世界核心子图：事件→世界→视图投影→数据驱动扇出（N 玩家），错误为值入账 | 小型 |
| [`redis_like/`](redis_like) | 真实子系统 | miniredis 硬化版：组合封闭单一端口体 + 三类物理驱动（inline/pump/link）+ 运行期换装 | 中型 |
| [`sqlmini/`](sqlmini) | 真实子系统 | SQL 子集编译链（词法→语法→计划→执行）+ Inline/并行双物理等价 | 仓库最大 |
| [`layered/`](layered) | 组织分层 | 库作者 / 拓扑集成方 / 部署方 三层分离，层间只经类型通信 | 结构 |
| [`control_seam/`](control_seam) | 控制/观测 | 控制是值（指令流）+ 运维控制面（暂停/换装）+ 观测三段式 | 结构 |

## 角色说明

- **概念演示**（3 个单文件）：回答"契约层与载体是什么"。`closed.rs` 一次展示
  foundations §8 的五个构造概念；`carrier_demo.rs` 与 `threaded_flow.rs` 从
  "换载体不改拓扑"与"同图多物理"两个方向演示 **T6 多物理语义等价**。
- **真实语义管线**（4 个）：把真实软件的分阶段语义纳入 axiom 词汇，验证
  "组合封闭 + 失败为值"能承载现实管线。`psql`/`netpath`/`mmo` 是小型管线；
  `redis_like`（miniredis）是首个"可信尺度"子系统，同一引擎三种物理驱动。
- **组织分层**（2 个）：`layered` 演示"库作者/集成方/部署方"的角色分离——
  真实多 crate 工作区即此结构的模块化拆分；`control_seam` 演示控制与观测
  作为值的共形写法（宪法 §8，B1/B4）。
- **综合用例**：跨层组合示例不在本目录——见工作区
  [`examples/sql-over-redis`](../../examples/sql-over-redis)（redis 协议面 ×
  psql 计算面在同一组合核心内协同，sync/async 双物理交叉验证）。`sqlmini` 与
  `redis_like` 是它的两个独立前身/组件源。

## 与 benches 的关系

benches 承担"实证"角色（零成本证据 / 回归闸门），示例承担"教学/范式证据"角色：

| 关注点 | 由谁回答 |
|---|---|
| 组合零成本（≈ 手写 Rust） | `core/benches/chain.rs`、`core/benches/dag.rs` |
| 载体时空成本、动态税 | `semantics/benches/carrier.rs`、`dynamic_tax.rs`、`profile_workloads.rs` |
| 综合用例延迟（sync vs async） | `examples/sql-over-redis/benches/latency.rs` |
| 概念是什么、怎么用 | 本目录示例 |
