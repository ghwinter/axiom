# axiom

**四构件编译期核心：开放系统 + 因果数据流 + 组合 + 静态性声明。**

Zero-dependency computation primitives for observable, controllable systems.
axiom 是一个**编译期模型**：蓝图用 Rust 代码/类型定义，核心能力到编译期耗尽
用于分析、验证，编译后等价手写普通 Rust、零运行时对象。

## 核心构件（`axiom::cell_core`）

| 构件 | 内容 | 编译期性质 |
|---|---|---|
| **开放系统/端口体** `PortCell` | 有边界、类型化输入/输出/状态、`step` 纯且内联 | 类型级，无运行时对象 |
| **因果数据流** `Link` | `A.out -> B.in`，类型层对偶配对 | 非法连接编译失败（T1） |
| **多对多** `Broadcast`(fan-out) / `Merge`(fan-in) | 广播、汇合，类型层强制 | 无 Tee 树 |
| **环** `Feedback` | 环的因果闭合在类型层表达 | 时序归物理载体（T3） |
| **组合** `CellChain` | 组合子仍是端口体，任意层级嵌套 | 操作类结构 |
| **静态性** `Static` / `DoesWire` / `assert_wiring` | 标记零成本子图 + 编译期布线验证 | 验证在编译期，运行期零开销 |

**核心承诺**：
- 蓝图即类型：零大小、运行时无对象（`size_of::<Blueprint<T>>()==0`）；
- 验证在编译期，运行期零开销；
- 编译后等价手写普通 Rust（见 `examples/cell_demo.rs`）。

**移出抽象层的旧语义（归物理载体）**：FlowKind（Data/Control/Observe）三分、
时序/Delay、线程/同步异步、值形态/JSON —— 详见 `docs/internal/theory`。

## runtime（`axiom-runtime`）

runtime 是核心的**物理层实现用例（载体 Carrier）**：为每条因果数据流提供"值如何
流动"的可替换物理方案——`InlineCarrier`（栈上函数传·零分配）、`QueueCarrier`/
`ChannelCarrier`/`spawned_flow`（堆队列/通道·跨线程）、`DirectCarrier`/`static_path`
（编译期展开）、`wire!` 声明宏。模块化、可替换，作未来 `axiom-tokio` 等第三方适配器
模板。

## 示例

| 文件 | 演示 |
|---|---|
| `examples/cell_demo.rs` | 四构件蓝图作为普通 Rust 程序运行（零运行时对象） |
| `examples/pipeline.rs` | 综合流水线：链 + 广播 + 反馈 + 编译期验证 |
| `runtime/examples/carrier_demo.rs` | 同一蓝图多载体可替换、语义等价、时空成本不同 |
| `runtime/examples/threaded_flow.rs` | 同拓扑异构物理：Inline 零分配 vs 跨线程通道 |

## 构建与验收

```text
cargo build --lib        # core（零依赖，no_std 支持 --no-default-features）
cargo test --lib         # 9 测试
cargo build/test --manifest-path runtime/Cargo.toml   # runtime（7 测试）
cargo run --example pipeline          # 跑示例
cargo run --manifest-path runtime/Cargo.toml --example threaded_flow
```

## 进一步阅读

- `docs/internal/theory/`：理论奠基（公理/定理 T1–T9）、编译期核心方向、统一抽象模型、
  重构计划（refactor-plan-compile-time-core / runtime-carriers）。
