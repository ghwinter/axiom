//! miniredis TCP 服务器 —— §9.3 IO 接缝（事件基座）的首个实现用例，纯 `std`（零第三方依赖）。
//!
//! 事件基座：外部 socket 字节流如何正式成为一条因果流的 `in`——
//!
//! ```text
//! TcpStream ──读──▶ LineSplit（每连接有状态缓冲）──行──▶ CmdParse（失败为值）
//!      │                                                      │
//!      │                                              有界通道 JOBS_CAP（背压）
//!      │                                                      ▼
//!      │                                         存储工作线程 DataStore（持有 StoreState）
//!      │                                                     │ RESP
//!      │                                                     ▼ StoreDemux
//!      └── 回写 ◀── 回执通道（队列顺序保持） ◀────────────────────────┘
//! ```
//!
//! ---------------- §9.3 事件基座载体类驱动（首案例）：字节块 → 行 → CmdParse → 有界回程 ----------------//
//!
//! 关键点：① 解析发生在连接线程（失败为值、短路，不触碰存储）；② 回程通道有界
//! （`JOBS_CAP`），存储工作线程忙时连接线程在投递处阻塞——背压经 TCP 滑动窗口上传；
//! ③ 回执以每连接 FIFO 保持应答顺序；④ 连接 EOF 后，回执通道关闭即触发写半关闭
//! （客户端读到完整应答序列后见 EOF）。存储工作线程全函数（解析已短路，无 panic 路径）。

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{SyncSender, channel, sync_channel, Receiver, Sender};
use std::thread;

use axiom::cell_core::PortCell;

use axiom_runtime::seams::event::{ChunkSource, PushVerdict, pump_events};

use crate::cells::{Cmd, CmdParse, DataStore, Error, LineSplit, StoreDemux, StoreState};

/// 回程通道容量：满则连接线程阻塞（背压）。
const JOBS_CAP: usize = 64;

type ReplyTx = Sender<String>;
type Job = (Result<Cmd, Error>, ReplyTx);

/// 存储工作线程（有状态、全函数：可失败解析已在连接侧短路为值）。
fn store_worker(mut store: StoreState, jobs: Receiver<Job>) {
    for (job, reply_tx) in jobs {
        let resp = match job {
            Ok(cmd) => StoreDemux::step(&mut (), DataStore::step(&mut store, cmd)),
            Err(e) => format!("-ERR {e}\r\n"),
        };
        let _ = reply_tx.send(resp); // 连接已断则忽略（回执通道随之关闭）
    }
}

/// 单连接处理：事件基座驱动——`ChunkSource`（块源 + 跨块行分割，每连接独立缓冲）
/// → `pump_events`（`CmdParse` 变换，失败短路计数）→ 有界回程（满则阻塞 = 背压）；
/// 消费端断连 ⟹ 泵停止拉取（拆除，不静默延续）；回执由专职写线程实时回写
/// （顺序 = 该连接入队顺序），EOF 后经通道关闭触发写半关闭。
fn handle_conn(stream: TcpStream, jobs_tx: SyncSender<Job>) {
    let (reply_tx, reply_rx) = channel::<String>();
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

    // 事件基座（§9.3 载体类）：字节块 → 行（跨块拼接，同 LineSplit 语义）→ CmdParse。
    let mut source = ChunkSource::<TcpStream, _, String, String, 1024>::new(
        stream,
        String::new(), // LineSplit 状态（每连接）
        |buf: &mut String, chunk: &[u8]| {
            let text = String::from_utf8_lossy(chunk).into_owned();
            LineSplit::step(buf, text)
        },
    );
    let _stats = pump_events::<CmdParse, _, _>(&mut (), &mut source, |parsed| {
        // 有界回程：满则阻塞（背压）；每条携带一份回执 Sender（FIFO）。
        // 存储线程已断连（拆除）⟹ Closed：泵停止拉取，本条计 dropped（诚实账，不静默）。
        if jobs_tx.send((parsed, reply_tx.clone())).is_err() {
            PushVerdict::Closed
        } else {
            PushVerdict::Delivered
        }
    });
    drop(reply_tx); // 本连接全部入队后释放回执源：队列剩余作业耗尽即关写半
    let _ = writer.join();
}

/// 服务器：接受循环（调用线程）→ 每连接一线程；存储工作线程持有状态（`--tcp PORT`）。
pub fn run_server(addr: &str, store: StoreState) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    let (jobs_tx, jobs_rx) = sync_channel::<Job>(JOBS_CAP);
    thread::spawn(move || store_worker(store, jobs_rx));
    eprintln!("miniredis: listening on {addr}（存储工作线程，回程容量 {JOBS_CAP} 有界背压）");
    for stream in listener.incoming().flatten() {
        let jobs_tx = jobs_tx.clone();
        thread::spawn(move || handle_conn(stream, jobs_tx));
    }
    Ok(())
}

/// 服务器自测（`--selftcp`）：临时端口起服务 → 客户端一次性命令序列 →
/// 返回按顺序收到的应答（含解析短路、背压、RESP 回写、半闭）。
pub fn selftest() -> Vec<String> {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let addr = listener.local_addr().expect("local addr");
    let (jobs_tx, jobs_rx) = sync_channel::<Job>(JOBS_CAP);
    let store = crate::cells::new_store(crate::cells::Config::default());
    thread::spawn(move || store_worker(store, jobs_rx));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let jobs_tx = jobs_tx.clone();
            thread::spawn(move || handle_conn(stream, jobs_tx));
        }
    });

    let mut client = TcpStream::connect(addr).expect("client connect");
    client
        .write_all(b"SET tcp_key 5\nGET tcp_key\nGET\nINCR tcp_key\nNOPE x\n")
        .expect("client write");
    let _ = client.shutdown(Shutdown::Write); // 半关：服务器读 EOF
    let mut replies = String::new();
    client.read_to_string(&mut replies).expect("read E2E");
    replies.lines().map(|s| s.to_string() + "\r\n").collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_io_seam_roundtrip() {
        let replies = selftest();
        assert_eq!(
            replies,
            vec![
                "+OK\r\n".to_string(), // SET tcp_key 5
                ":5\r\n".to_string(), // GET tcp_key
                "-ERR GET requires a key\r\n".to_string(), // GET（解析短路，存储未触碰）
                ":6\r\n".to_string(), // INCR tcp_key
                "-ERR unknown command 'NOPE'\r\n".to_string(), // NOPE（解析短路）
            ]
        );
    }
}