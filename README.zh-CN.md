# axiom

**四构件编译期核心：开放系统 + 因果数据流 + 组合 + 静态性声明。**

[English](README.md) · [简体中文](README.zh-CN.md)

> **新读者？从[导览](docs/zh-cn/primer.md)开始（双语：[en](docs/en-us/primer.md) /
> [zh-cn](docs/zh-cn/primer.md)）**——axiom 是什么、不是什么、愿景几行、以及无框架时如何
> 思考。三句话：(1) 你写"什么连接什么"（拓扑）用的是**类型**——非法布线编译不过；
> (2) 你声明"谁必须做什么"（期限 / 取消 / 背压）用的是**声明项**，不是注释；(3) 你选择
> "等待与 I/O 如何实现"（物理）的方式是**换一个载体**——拓扑不变。axiom 是**宪法层，
> 不是框架**：拓扑安全 + 义务显式 + 物理可换，不给你系统。

axiom 是一个**宪法层，不是框架**：它提供类型化词汇（形状、契约、义务模态）、编译期验证
与可替换物理接缝——不是应用、不是 all-in-one 运行时、无控制反转、无生命周期托管。Rust
给你内存安全但不给你应用；axiom 给你拓扑安全、义务显式、物理可换但不给你系统。

可观测、可控制系统的零依赖计算原语。axiom 是一个**编译期模型**：蓝图在 Rust 代码/类型中
定义，核心的智能在编译期耗尽（分析与验证）。编译之后等价于手写普通 Rust，零运行时对象。

## 核心构件（`axiom::cell_core`）

| 构件 | 内容 | 编译期性质 |
|---|---|---|
| **开放系统 / 端口单元** `PortCell` | 有界、类型化输入/输出/状态，`step` 纯且内联 | 类型级，无运行时对象 |
| **因果数据流** `Wire` | `A.out -> B.in`，类型级对偶配对 | 非法布线编译不过（T1） |
| **多对多** `Broadcast`（扇出）/ `Merge`（汇合） | 扇出、汇合，类型级强制 | 无 Tee 树 |
| **环** `Feedback` | 类型级表达环的因果闭包 | 时序属物理载体（T3） |
| **组合** `Chain` | 组合子本身是端口单元，可嵌套任意深度 | Operad 结构 |
| **静态性** `Static` / `Conforms` / `assert_wiring` | 标记零成本子图 + 编译期布线验证 | 验证在编译期，运行期零开销 |

**核心承诺**：
- 蓝图即类型：零大小、无运行时对象（`size_of::<Blueprint<T>>()==0`）；
- 验证在编译期，运行期零开销；
- 编译后等价于手写普通 Rust（见 `core/examples/cell_demo.rs`）。

**语义标注，非物理机制**：FlowKind（Data/Control/Observe）是可选抽象层标注，描述接收方
如何解释一个值——不是物理层属性。物理层把所有值一律当作"流经结构的值"（共享变量 / 缓冲 /
通道）。时序/延迟、线程化/同步-异步、值形态/JSON 仍是物理层关切——见
`docs/zh-cn/foundations.md` §5.8。

## semantics（`axiom-semantics`）

semantics 是**契约层**（语义函子 ⟦core 形状范畴⟧ → 行为范畴）：为每条因果数据流声明行为
与时空成本契约——三个等待点契约（输入就绪 / 期限 / 背压）加激活契约，以及载体插座。
库内载体：`InlineCarrier`（栈调用 · 零分配）、`QueueCarrier` / `BoundedCarrier<CAP>`
（堆队列 / 有界通道）、`spawned_flow`（通道 + 专用线程 · 跨线程、工作线程 panic 经回执
传播），以及 `wire!` 声明宏。模块化可替换：新载体实现 `Carrier` trait 即可接入，不改拓扑；
真实基座（tokio / io_uring / std / embedded）由实例层绑定兑现。

## 四边接缝与洞清单

运行期边界是**四边接缝地图**，不是三岔信息流。Data/Control/Observe 是*接缝方向*的分类，
不是信息的分类——物理边证明了这一点：它根本不携带信息（分配器是物质）。

| 边 | 接缝 | 表面 |
|---|---|---|
| Data | `Wire`（core）/ `Carrier`（semantics） | In/Out/State |
| Control | `Executor`（`async-seam`） | 等待点契约 |
| Observe | `Telemetry`（`telemetry`） | 逐接缝裁决 |
| **Physical** | `seams::physical`（`PhysicalWindow` / `PhysicalSnapshot`） | **无表面**——进程单例窗（分配器 / 信号 / stdio），只声明与观测，永不是 In/Out/State |

**横切**接缝（`seams::crosscut`，`CrossCut` 标记）是按设计无面的唯一变体：它随调用流移动、
不属于任何模块（上下文、取消令牌）。它不是单元，永不进入 `assert_wiring`。

**洞清单**（`checks::friction`）是治理产物，不是词汇扩展：代数表达不了的六种摩擦（等待 /
物理单例 / 横切 / 常驻 / 派生语义 / 完备性）被类型化、索引到它们的承载接缝、并保持
**开放**（`#[non_exhaustive]`）——完备性无法在律内自证，所以新洞*加入清单*，而不是假装没有。

## instances（`axiom-instances` · 第三构件）

**实例层**经接缝（`Executor` / `Carrier` / `Telemetry`）把可替换的物理/生态实现插进核心。
官方实例以一个融合 crate 交付，全部特性默认关闭；第三方自建独立 crate（双形边界——融合
标准集 vs 开放路径）。

| feature | 引入 | 提供 |
|---|---|---|
| `async` | `axiom-semantics/async-seam` | 异步接缝（`Executor` 契约） |
| `tokio` | `async` + 可选 `tokio` 依赖 | `TokioExec`：面向 tokio 的接缝等待点适配 |
| `embedded` | `axiom-semantics/std` | 保留的嵌入式流 |

依赖方向单向，由 workspace 成员表强制：`axiom ← axiom-semantics ← axiom-instances`。核心保持
零依赖承诺；语义层只加一个编译期依赖（`axiom-macros` proc-macro crate，运行时不链接任何东西）；
`tokio` 只作为 instances 的可选依赖存在。

## 示例

> 契约层示例索引：[`semantics/examples/README.md`](semantics/examples/README.md)
> （角色、尺度、与综合用例的关系）。全部 semantics 示例以 `--manifest-path semantics/Cargo.toml` 运行。

| 文件 | 演示 |
|---|---|
| `core/examples/cell_demo.rs` | 四构件蓝图作为普通 Rust 程序运行（零运行时对象） |
| `core/examples/pipeline.rs` | 组合管道：chain + broadcast + feedback + 编译期验证 |
| `core/examples/deep_blueprint.rs` | 蓝图深度 × 编译成本诚实探针（采纳证据：编译时间随深度增长，运行期保持为零） |
| `semantics/examples/closed.rs` | 封闭边界集成（foundations §8）：组合自封闭、失败即值、T6、运行期槽位换装 |
| `semantics/examples/carrier_demo.rs` | 同一蓝图、多个可替换载体、语义等价、时空成本不同 |
| `semantics/examples/threaded_flow.rs` | 同一拓扑、异构物理：内联零分配 vs 跨线程通道 |
| `semantics/examples/psql/` | SQL REPL：单层 `TryChain` 短路（lexer/parser/exec 错误即值） |
| `semantics/examples/netpath/` | 网络接收路径：Eth→IP→TCP 解析，双载体短路等价（T6） |
| `semantics/examples/mmo/` | 多玩家世界核心子图：events→world→view 投影→数据驱动扇出 |
| `semantics/examples/redis_like/` | miniredis 加固：封闭组合单端口体 + 三个物理驱动 + 运行期换装 |
| `semantics/examples/sqlmini/` | SQL 子集编译链（lex→parse→plan→exec）+ Inline/分区并行双物理等价（最大示例） |
| `semantics/examples/layered/` | 三角色分层：库作者 / 拓扑集成者 / 部署者（契约 = `PortCell` 类型） |
| `semantics/examples/control_seam/` | 控制与观测符合：控制即值 + 操作控制面 + 观测三元组 |
| `instances/examples/tokio_timeout_async.rs` | tokio 真异步超时：多线程 reactor 上的 await 驱动轮询（需 `--features tokio`） |
| `examples/sql-over-redis/` | 综合用例：redis 协议面 × psql 计算面在同一组合核心（同步/异步并发演示） |

## 基准

基准是**经验证据**层（零成本证明 / 回归闸门）；示例是教学/范式证据层。全部基准仅 release
（debug 构建下自我跳过）。

| 基准 | 回答 |
|---|---|
| `core/benches/chain.rs` | 组合零成本：泛型驱动 ≈ 手写 `step` 链（逐位一致） |
| `core/benches/dag.rs` | 菱形（串后并）泛型 vs 手写 vs 类型擦除 |
| `semantics/benches/carrier.rs` | 载体时空成本：内联零分配 vs spawned 跨线程通道 |
| `semantics/benches/dynamic_tax.rs` | 动态税：每驱动类型槽/席位擦除成本、换装成本 |
| `semantics/benches/profile_workloads.rs` | 形状工作负载画像（min-of-N 方法论） |
| `examples/sql-over-redis/benches/latency.rs` | 综合用例延迟：同步 vs 异步逐步、同一协议（需 `--features tokio`） |

## 构建与验证

```text
cargo build --workspace                    # core + semantics + instances + 用例 crates
cargo test --workspace                     # core + semantics + 演示单测/集成
cargo bench -p axiom --bench dag              # 菱形零成本证明（组合 ≈ 手写，Δ≈±1%）——仅 release 证据
cargo build -p axiom-instances --features tokio   # 实例层（tokio 特性门控；默认全关）
cargo test -p axiom-instances --features tokio    # 实例层 + 等价对拍（T6 多物理语义等价）
cargo run -p axiom-demo-sql-over-redis --bin sync_demo          # 综合用例，同步演示（SQL-over-Redis；零第三方）
cargo run -p axiom-demo-sql-over-redis --features tokio --bin async_demo  # 异步变体（真异步馈入驱动 + 观测子系统）
cargo run -p axiom-demo-sql-over-redis --features tokio --bin concurrent_demo  # 并发等待量化（1 线程服务 N 会话）
cargo bench -p axiom-demo-sql-over-redis --features tokio --bench latency     # 同步 vs 异步逐步骤延迟，同一协议（min-of-N）
cargo run -p axiom --example pipeline              # 运行示例（core 包）
cargo run -p axiom-semantics --example threaded_flow
```

`--workspace` 经根 `Cargo.toml` `[workspace]` 统一解析（合并了旧双清单分裂）；单一
`Cargo.lock`/`target`。no_std 承诺：`cargo build -p axiom --no-default-features`、
`cargo build -p axiom-semantics --no-default-features`（实例层不参与 no_std）。

> 基准只在 release 配置下有意义；debug 构建下它们自我跳过，而非输出误导数字。

## 延伸阅读

- [`docs/`](docs/README.md)：正式规范（双语，英文默认）——`foundations.md`（定义/公理/定理
  T1–T9）、`core.md`（编译期核心 `cell_core`）、`semantics.md`（物理层 / 载体）。
