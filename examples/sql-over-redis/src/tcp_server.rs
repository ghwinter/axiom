//! TCP 服务器（网络异步 I/O）——同一组合核心，sync/async/仿真三物理域服务。
//!
//! 把事件接缝（§9.3）落到真实套接字：`SQL-over-Redis` 组合核心经 TCP 服务器对外服务。
//! 每个连接一个事件泵（字节块 → 行分割 → [`RouteParse`]，解析失败为值）；共享组合
//! 状态（[`CompositeState`]）由存储任务单属主持有；有界 jobs 通道背压；应答按序回写。
//! 同一蓝图实现多次（T6 多物理实现语义等价）：
//!
//! - **sync 域**（`std`，默认可用）：每连接线程 + [`ChunkSource`]/[`pump_events`]，
//!   `push` 在线程上阻塞投递（满 = 背压）——形状与首案例 `redis_like --tcp` 一致；
//! - **async 域**（`tokio` feature）：每连接任务 + [`AsyncLineSource`]/
//!   [`pump_events_async`]，`push` 是异步闭包（`send().await` 满 = 挂起背压），
//!   等待点接入 tokio reactor——首案例的异步落点；
//! - **仿真域**（`turmoil` feature，确定性仿真第三物理域）：**同一 `serve_tcp_async` 蓝图**，
//!   仅 [`crate::net`] 处编译期切换 `tokio::net` → `turmoil::net`（确定性仿真载体：
//!   单线程模拟主机/时间/网络，`rng_seed` 可复现对抗迹）——见
//!   `tests/tcp_sim_adversarial.rs`。
//!
//! ## 架构（三域同形，差异只在物理兑现）
//!
//! ```text
//! 连接 k: 套接字读半 ─块─▶ 事件泵（行分割 → RouteParse，失败为值）
//!               │ push：有界 jobs 通道（JOBS_CAP，满则背压）────────┐
//!               ▼                                                 ▼
//!         存储任务/线程（共享 CompositeState 单属主）─ExecCell─▶ 按序回执通道
//!                                                                  │
//!                                                     连接 k 写回任务/线程 ─▶ 写半
//! ```
//!
//! ## 背压诚实性（两级有界，无静默丢值）
//!
//! 客户端不读 → 写回阻塞（sync: 线程阻塞写 / async: `write_all().await` 挂起）→
//! 回执通道缓冲（该连接局部，不拖累其它连接）→ jobs 通道满 → 事件泵停止拉取
//! （sync: `send` 阻塞 / async: `send().await` 挂起）→ 不再读套接字 → TCP 滑动
//! 窗口上行。跨连接背压只有 jobs 一个闸（同首案例 `JOBS_CAP`）。
//!
//! ## 义务（事件接缝，测试见证）
//!
//! - **配对律**：判定之和 = 泵拉取总数（线上可观测形态：每条命令恰一个应答）；
//! - **失败归属**：解析错误为值，经 `-ERR` 转发（泵不短路吞值）；
//! - **拆除语义**：客户端断连 → 泵停止拉取（dropped 诚实计数，不静默延续）；
//! - **按序回执**：每连接应答顺序 = 该连接入队顺序（存储全局 FIFO）。
//!
//! 系统级 T6 交叉验证（sync/async 服务器 × inline 组合迹三方零分歧）见
//! `tests/tcp_t6_crosscheck.rs`；可运行演示见 `bin/tcp_demo.rs`。
//!
//! 依赖方向单向：用例 → `axiom-instances`（[`AsyncLineSource`]，tokio feature）。

use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{Sender as StdSender, SyncSender, channel as std_channel, sync_channel};
use std::thread;

use axiom::cell_core::PortCell;
use axiom_semantics::seams::event::{
    ChunkSource, PushVerdict, pump_events, split_lines,
};

use crate::composite::{CompositeState, Route, RouteParse, ExecCell};
use crate::plans::redis_plan::Error;

/// jobs 通道容量：满则连接侧事件泵阻塞/挂起（背压，跨连接共享；同首案例 `JOBS_CAP`）。
pub const JOBS_CAP: usize = 64;

/// 行分割器（与首案例 `LineSplit` 同语义）：`&[u8]` 块 → 行（去空白，跨块拼接）。
pub fn split_socket(buf: &mut String, chunk: &[u8]) -> Vec<String> {
    split_lines(buf, chunk)
}

// ════════════════════════ sync 域（std，默认可用） ════════════════════════

/// 存储工作线程：共享组合状态单属主；解析错误为值转发 `-ERR`（不触碰存储）。
fn store_worker_sync(mut store: CompositeState, jobs: std::sync::mpsc::Receiver<JobSync>) {
    for (parsed, reply_tx) in jobs {
        let resp = match parsed {
            Ok(route) => ExecCell::step(&mut store, route),
            Err(e) => format!("-ERR {e}\r\n"),
        };
        let _ = reply_tx.send(resp); // 客户端已断则忽略（回执通道随之关闭）
    }
}

/// sync 单连接：事件泵（[`ChunkSource`] + [`pump_events`]）→ 有界 jobs 投递
/// （阻塞 = 背压）→ 按序回执 → 写回线程。消费端断连 ⟹ 泵停止拉取（拆除）。
fn handle_conn_sync(stream: TcpStream, jobs_tx: SyncSender<JobSync>) {
    let (reply_tx, reply_rx) = std_channel::<String>();
    let writer_stream = stream.try_clone().expect("TcpStream clone");
    let writer = thread::spawn(move || {
        let mut w = writer_stream;
        for resp in reply_rx {
            if w.write_all(resp.as_bytes()).is_err() {
                break;
            }
        }
        let _ = w.shutdown(Shutdown::Write); // 回执通道关闭 → 写半关闭 → 客户端 EOF
    });

    let mut source = ChunkSource::<TcpStream, _, String, String, 1024>::new(
        stream,
        String::new(), // 行分割状态（每连接）
        split_socket,
    );
    let _stats = pump_events::<RouteParse, _, _>(&mut (), &mut source, |parsed| {
        // 有界回程：满则阻塞（背压）；存储线程已断连（拆除）⟹ Closed：泵停止拉取。
        if jobs_tx.send((parsed, reply_tx.clone())).is_err() {
            PushVerdict::Closed
        } else {
            PushVerdict::Delivered
        }
    });
    drop(reply_tx); // 本连接全部入队后释放回执源：队列剩余作业耗尽即关写半
    let _ = writer.join();
}

/// sync 服务器：接受循环（调用方线程）→ 每连接一线程；存储工作线程持有共享状态。
/// `listener` 关闭（丢弃）⟹ `accept` 出错 ⟹ 正常返回。
pub fn serve_tcp_sync(listener: TcpListener, store: CompositeState) {
    let (jobs_tx, jobs_rx) = sync_channel::<JobSync>(JOBS_CAP);
    thread::spawn(move || store_worker_sync(store, jobs_rx));
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let jobs_tx = jobs_tx.clone();
                thread::spawn(move || handle_conn_sync(stream, jobs_tx));
            }
            Err(_) => break, // 监听器已关闭
        }
    }
}

/// sync 域作业：解析结果（失败为值）+ 该连接的回执 Sender。
type JobSync = (Result<Route, Error>, StdSender<String>);

// ════════════════ async 域（tokio / turmoil feature 门控，net 类型经 crate::net） ════════════════

#[cfg(any(feature = "tokio", feature = "turmoil"))]
pub use async_server::serve_tcp_async;

#[cfg(any(feature = "tokio", feature = "turmoil"))]
mod async_server {
    use super::*;
    use crate::net::{TcpListener, TcpStream};
    use axiom_instances::backend::async_event::AsyncLineSource;
    use axiom_semantics::seams::event::pump_events_async;
    use tokio::io::{AsyncWriteExt};
    use tokio::sync::mpsc::{UnboundedSender, channel as async_channel};

    /// async 域作业：解析结果（失败为值）+ 该连接的回执 UnboundedSender。
    type JobAsync = (Result<Route, Error>, UnboundedSender<String>);

    /// 存储任务：共享组合状态单属主；解析错误为值转发 `-ERR`（不触碰存储）。
    async fn store_task_async(
        mut store: CompositeState,
        mut jobs: tokio::sync::mpsc::Receiver<JobAsync>,
    ) {
        while let Some((parsed, reply_tx)) = jobs.recv().await {
            let resp = match parsed {
                Ok(route) => ExecCell::step(&mut store, route),
                Err(e) => format!("-ERR {e}\r\n"),
            };
            let _ = reply_tx.send(resp); // 客户端已断则忽略（回执通道随之关闭）
        }
    }

    /// async 单连接：事件泵（[`AsyncLineSource`] + [`pump_events_async`]）→ 有界
    /// jobs 投递（`send().await` 满 = 挂起背压，等待点挂 reactor）→ 按序回执 →
    /// 写回任务。消费端断连 ⟹ 泵停止拉取（拆除）。
    async fn handle_conn_async(stream: TcpStream, jobs_tx: tokio::sync::mpsc::Sender<JobAsync>) {
        let (read_half, write_half) = stream.into_split();
        let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        // 写回任务：按序 await 回执 → 写半；回执通道关（本连接全部入队且耗尽）→ 关写半。
        let writer = tokio::spawn(async move {
            let mut w = write_half;
            while let Some(resp) = reply_rx.recv().await {
                if w.write_all(resp.as_bytes()).await.is_err() {
                    break;
                }
            }
            let _ = w.shutdown().await; // 客户端读半 EOF → 写半关闭 → 客户端 EOF
        });

        let mut source = AsyncLineSource::<_, _, String, String>::new(
            read_half,
            String::new(), // 行分割状态（每连接）
            split_socket,
            1024,
        );
        // 属主克隆移入 async move 闭包（future 无借用依赖 → Send 一般化成立）；
        // reply_tx 由闭包持有，泵结束（闭包析构）即释放回执源 → 写回任务耗尽后关写半。
        let jobs_tx = jobs_tx.clone();
        let reply_tx = reply_tx;
        let _stats = pump_events_async::<RouteParse, _, _>(&mut (), &mut source, async move |parsed| {
            // 有界回程：满则 await（背压）；存储任务已断（拆除）⟹ Closed：泵停止拉取。
            if jobs_tx.send((parsed, reply_tx.clone())).await.is_err() {
                PushVerdict::Closed
            } else {
                PushVerdict::Delivered
            }
        })
        .await;
        let _ = writer.await;
    }

    /// async 服务器：接受循环（调用方任务）→ 每连接一任务；存储任务持有共享状态。
    /// `listener` 关闭（丢弃）⟹ `accept` 出错 ⟹ 正常返回。
    ///
    /// `TcpListener` 经 [`crate::net`] 编译期切换：实际 tokio 域 / turmoil 仿真域
    /// （确定性仿真第三物理域，同一蓝图零改动）。
    pub async fn serve_tcp_async(
        listener: TcpListener,
        store: CompositeState,
    ) -> std::io::Result<()> {
        let (jobs_tx, jobs_rx) = async_channel::<JobAsync>(JOBS_CAP);
        tokio::spawn(store_task_async(store, jobs_rx));
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    let jobs_tx = jobs_tx.clone();
                    tokio::spawn(handle_conn_async(stream, jobs_tx));
                }
                Err(_) => return Ok(()), // 监听器已关闭
            }
        }
    }
}

// ════════════════════════ 客户端驱动器（测试/演示共用，物理域无关） ════════════════════════

/// 标准客户端：把 `lines` 一次性写入（行以 `\n` 结尾）→ 半关写 → 读到 EOF →
/// 按 `\r\n` 拆回执（去尾空串）。返回的应答数 = 命令数（线上配对律的可观测形态）。
pub fn client_run(addr: std::net::SocketAddr, lines: &[String]) -> Vec<String> {
    use std::io::{Read, Write};
    let mut c = TcpStream::connect(addr).expect("client connect");
    c.set_read_timeout(Some(std::time::Duration::from_secs(15)))
        .expect("read timeout");
    let payload = lines.join("\n") + "\n";
    c.write_all(payload.as_bytes()).expect("client write");
    let _ = c.shutdown(Shutdown::Write); // 半关：服务器读 EOF
    let mut all = String::new();
    c.read_to_string(&mut all).expect("read E2E");
    all.split("\r\n").filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
}
