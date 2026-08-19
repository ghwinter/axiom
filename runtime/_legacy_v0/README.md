# `runtime/_legacy_v0/` — 旧 runtime（v0）搁置区

> **性质**：本次 runtime 重建中被移出编译面的旧代码，**保留作物理实现思路参考**，不编译。
> **为何搁置**：旧 runtime 依赖已被移出的 v0 core（DynamicTopology/Machine/LinkKind），
> 接口与"四构件编译期核心（cell_core）"不匹配。物理实现思路（carrier 载体、typed_slot
> 零分配级间、fusion 链、IO 反应器 epoll/kqueue/wsa）在此保留，供重建时择机参考。
>
> - `runtime_lib_v0.rs`：旧 runtime crate 入口文档。
> - `carrier.rs` / `typed_slot.rs` / `fusion.rs` / `static_path.rs`：物理流动/零分配/融合思路。
> - `io__*.rs`：IO 多路复用平台实现（epoll/kqueue/wsa）。
> - `runtime.rs`/`routing.rs`/`scheduler.rs`/`topology.rs`/`registry.rs`/`erasure.rs`/`config.rs`/
>   `contract.rs`/`error.rs`/`replay.rs`/`tests.rs`：旧驱动/行为。

## 新 runtime（当前 `runtime/src/`）

重建为 **cell_core 的物理层实现用例（载体 Carrier）**——见
`docs/internal/theory/refactor-plan-runtime-carriers.md`。
