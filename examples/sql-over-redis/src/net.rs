//! 网络物理切换（确定性仿真载体 ↔ 实际 tokio）。
//!
//! 同一 async 服务器蓝图经**编译期切换**落到两个物理域（turmoil 官方推荐形态：
//! `#[cfg]` 处切 `tokio::net` ↔ `turmoil::net`，镜像 API）：
//!
//! ```text
//! feature 关闭（tokio/turmoil 均开）：tokio::net::TcpListener / TcpStream（实际域）
//! feature 开启（turmoil 打开）    ：turmoil::net::TcpListener / TcpStream（仿真域）
//! ```
//!
//! 服务器代码零改动：`tcp_server::serve_tcp_async` 在本模块处取类型，其它一切
//! （事件泵 / 有界背压 / 按序回写）共享同一实现——"同一蓝图、多物理实现"（T6）
//! 在网络域的直接兑现。turmoil 用 tokio 1.x 的运行时代理每个主机（`tokio::spawn`/
//! `tokio::sync` 在主机内照常可用），其流类型实现 tokio 的 `AsyncRead`/`AsyncWrite`，
//! 故事件接缝（`AsyncLineSource` 需 `AsyncRead + Unpin + Send`）无需改动。
//!
//! 依赖方向单向：用例 → turmoil（仅 feature 门控下）。

#[cfg(not(feature = "turmoil"))]
pub(crate) use tokio::net::{TcpListener, TcpStream};

#[cfg(feature = "turmoil")]
pub(crate) use turmoil::net::{TcpListener, TcpStream};
