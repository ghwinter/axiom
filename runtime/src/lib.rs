//! # axiom-runtime
//!
//! axiom 的统一运行时：将 `DeploySpec` 物化为活跃的 `MachineHandle`，
//! 驱动 `process` 循环，管理生命周期。
//!
//! ## 设计原则
//!
//! - **统一 runtime，配置区分模式**：单线程与多线程不是两个独立类型，
//!   而是同一个 `Runtime` 在 `RuntimeConfig::mode` 上的不同取值。
//!   `Inline` → 调用方线程内联执行；
//!   `Sequential` → 单线程顺序循环；
//!   `Parallel(n)` → N 个 worker 线程并行调度。
//! - **native loop 不可自举**：runtime 的驱动循环本身不是 Machine，
//!   是一段 C 风格的 `loop { pull; process; route }`。
//! - **process 保持同步**：`Machine::process` 的同步签名不变。IO 多路复用、
//!   线程池管理是 runtime 的职责，不污染 core 的纯契约层。
//! - **静态拓扑优先**：runtime 物化 `DeploySpec` 后，拓扑在内存中固定，
//!   不支持运行时增删 Machine。需要"看起来动态"的行为（弹性、路由），
//!   用静态拓扑 + Machine 内部 State 变化表达。
//!
//! ## 覆盖范围
//!
//! - 单线程顺序驱动循环（`Sequential` 模式，直接 move 投递）
//! - 多线程驱动（`Parallel(n)` 模式：每机器一个 OS 线程，链接按
//!   `LinkKind` 物化为 `mpsc::channel` / `mpsc::sync_channel` /
//!   自定义有界覆盖 / 单槽覆盖载体；channel 断开级联停机）
//! - `RegisterFn` 注册表 + 类型擦除 `RunningMachine`
//! - `materialize` / `tick` / `shutdown` 生命周期
//! - **output → input 路由**（tick 按 `LinkSpec` 把输出投递到下游，
//!   BFS 逐级传播，含 Tee fan-out）
//! - **停机传播**（`Done` = 停机信号：机器停机、积压丢弃、级联传播到
//!   所有入边源均已停机的下游；Parallel 线程收到 `Done` 立即退出）
//! - **fan-in 支持**（Parallel 模式多入边经 forward 线程合并消费，
//!   按到达顺序注入）
//! - **B 档载体**：`Overwriting` 有界覆盖（满时覆盖最老）、`Latest`/
//!   `SharedState` 单槽覆盖、`ReadPolicy::NonBlocking` 轮询
//! - **pipelineN 融合**：`materialize` 自动识别相邻 `FusedInline` 机器
//!   的 Inline 链，替换为 `FusedPipeline`——消除每跳的路由查找
//!   （2 次 String 克隆），每跳从 +4 降到 +2 alloc（R003）
//! - **复合 Machine**：`register_composite` 把子拓扑 + 端口映射封装为
//!   单一 `machine_type`；`materialize` 递归展开（名字空间化子机器 +
//!   重定向外部链接），展开在融合之前——`FusedPipeline` 可跨原复合
//!   边界融合
//!
//! 未覆盖（后续增量）：
//! - `CasFreeRing` 的无锁固定地址载体（嵌入式场景；runtime 迁移为无界
//!   channel）；`SharedState` 的多读者语义（当前单消费者近似）
//! - 编译期 `pipelineN` 泛型函数（彻底消除 Box，需具体类型；当前
//!   runtime 融合在类型擦除层，保留 Box<dyn Any>）
//! - Windows 大规模 IO 的 IOCP completion 模型（当前 WSAEventSelect
//!   readiness 模型支持 ≤64 源；生产级数千连接需 IOCP）
//!
//! ## 模块结构
//!
//! - [`config`] — `ExecMode` / `RuntimeConfig`
//! - [`erasure`] — `RunningMachine` trait + `ProcessResult` + `MachineWrapper`
//! - [`registry`] — `RegisterFn` + `Registry`
//! - [`topology`] — `LiveTopology` + `PhysicalLink`
//! - [`carrier`] — Parallel 链接载体（`ChanSender`/`ChanReceiver` + 覆盖/单槽实现）
//! - [`routing`] — 路由 + 停机传播 + 端点校验 + 环检测
//! - [`fusion`] — pipelineN 融合（`FusedPipeline` + 链识别 + `apply_fusion`）
//! - [`io`] — IO 多路复用（`IoReactor` trait + epoll/kqueue/WSAEventSelect 平台实现）
//! - [`runtime`] — `Runtime` 主体（`materialize`/`tick`/`shutdown`/`run_io`）

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
compile_error!("axiom-runtime currently requires the `std` feature (std::sync::mpsc, std::thread)");

extern crate alloc;

mod carrier;
mod config;
mod erasure;
mod error;
mod fusion;
mod io;
mod registry;
mod routing;
mod runtime;
mod static_path;
mod topology;

#[cfg(test)]
mod tests;

// 公共 API re-export
// CompositeSpec / expand_composites / CompositeError 现在从 core 引用——
// 复合是结构定义能力，属于 axiom core；runtime 只在物化时调用 expand_composites。
pub use axiom::composite::{CompositeSpec, CompositeError, expand_composites};
pub use axiom::static_exec::{CloneSplit, IdLink, Link, Merge, Split, StaticExecError};
pub use config::{ExecMode, RuntimeConfig};
pub use erasure::{ProcessResult, RunningMachine};
pub use error::RuntimeError;
pub use io::{
    IoError, IoEvent, IoInterest, IoReactor, IoToken, ManualReactor, RawIo, DefaultReactor,
    default_reactor,
};
pub use registry::{RegisterFn, Registry};
pub use runtime::Runtime;
pub use static_path::{fanin2, fanout2, pipeline2, pipeline3};
pub use topology::{LiveTopology, PhysicalLink};
