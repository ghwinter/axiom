# `_legacy_v0/` — 旧核心（v0）搁置区

> **性质**：本次"核心清洁"重构中被移出编译面的旧代码，**保留作历史参考**，不被编译。
> **为何搁置而非删除**：这些文件是旧 axiom（Func+Machine、FlowKind 三分、值形态蓝图、
> runtime 适配）的成果；重构方向是"四构件编译期核心"，但删除旧代码前保留能避免不可逆损失、
> 并可在此回归对照。**用例代码已搁置、暂不编译**。

## 内容

- `src/_legacy_v0/*.rs`：旧核心模块（analysis/backpressure/blueprint/compat/composite/
  config/deploy/entity/flow/func/hybrid/link/lint/machine/migrate/port/portset/projection/
  resource/runtime_contract/session/shared/static_exec/stream/time/topology）。
- `src/_legacy_v0/examples/`：旧用例（declarative_dag/graph_validation/http_tutorial/
  threaded_pipeline/verify_derive_zero_cost/psql）。

## 新核心（当前编译面 `src/`）

- `src/lib.rs`：四构件主轴线 crate 入口。
- `src/cell_core.rs`：开放系统/因果数据流/组合/静态性声明 + 编译期验证（DoesWire）。

## 下一步（用户指定路径）

1. 核心已清洁（本步完成）。
2. **runtime 重建**：把 runtime 从"旧 model 适配"改为 cell_core 的**物理层实现用例**
   ——载体（值如何流动：栈上传/堆队列/inline/编译期展开）、宏、模块化、可替换；
   未来可做 axiom-tokio。
3. 从 `_legacy_v0` 择机提取有用的旧概念（如 session 协议对偶）映射进新核心，
   或删除不再需要的。
