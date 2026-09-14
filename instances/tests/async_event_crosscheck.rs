#![cfg(feature = "tokio")]
//! T6 交叉验证器械（事件接缝）：同一事件流蓝图在同步域与异步域的迹零分歧。
//!
//! 蓝图 = 事件接缝（§9.3）：`原始块源 → 分割器 → 条目 → pump → A::step → push 裁决`。
//! 两个物理域：
//!
//! - **同步域**：语义层 [`ChunkSource`](axiom_semantics::seams::event::ChunkSource)
//!   （`std::io::Read` 块源）＋ [`pump_events`]；
//! - **异步域**：实例层 [`AsyncLineSource`](crate::backend::async_event::AsyncLineSource)
//!   （tokio `AsyncRead` 块源，等待点挂 reactor）＋ [`pump_events_async`]。
//!
//! 判据（T6 多物理实现语义等价）：同字节输入序列 → 同条目序列 → 同变换输出
//! 序列 → 同裁决序列（`delivered`/`dropped` 同账）。覆盖三景：全序列、跨块切割
//! 与 EOF 残留、拆除语义（消费端断连）。

use axiom::cell_core::PortCell;
use axiom_instances::backend::async_event::{AsyncLineSource, pump_events_async};
use axiom_semantics::prelude_all::{
    ChunkSource, EventPumpStats, PushVerdict, pump_events, split_lines,
};
use std::io::{self, Read};

// ── 被测格：行 → Result<i64, &'static str>（失败也是数据，同 event.rs 测试格）──

struct ParseI64;
impl PortCell for ParseI64 {
    type In = String;
    type Out = Result<i64, &'static str>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), line: String) -> Result<i64, &'static str> {
        line.trim().parse::<i64>().map_err(|_| "bad")
    }
}

/// 同步分块读取器：按指定块序列喂给 reader（块内跨多次读，模拟真实到达）。
struct SyncChunked {
    chunks: Vec<&'static [u8]>,
    at: usize,
    pos: usize,
}
impl SyncChunked {
    fn new(chunks: Vec<&'static [u8]>) -> Self {
        Self {
            chunks,
            at: 0,
            pos: 0,
        }
    }
}
impl Read for SyncChunked {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.chunks.len() {
            return Ok(0); // EOF
        }
        let chunk = self.chunks[self.at];
        if self.pos >= chunk.len() {
            self.at += 1;
            self.pos = 0;
            return self.read(out);
        }
        let n = (chunk.len() - self.pos).min(out.len());
        out[..n].copy_from_slice(&chunk[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// 异步分块读取器：同一块序列，每次 poll_read 交付一整块。
struct AsyncChunked {
    chunks: Vec<&'static [u8]>,
    at: usize,
}
impl tokio::io::AsyncRead for AsyncChunked {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        out: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.at >= this.chunks.len() {
            return std::task::Poll::Ready(Ok(())); // EOF（无数据可填）
        }
        let chunk = this.chunks[this.at];
        let n = chunk.len().min(out.remaining());
        out.put_slice(&chunk[..n]);
        if n == chunk.len() {
            this.at += 1;
        }
        std::task::Poll::Ready(Ok(()))
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("current-thread rt")
        .block_on(f)
}

/// 同步域迹：块源 → 条目序列 → 变换输出 → 裁决账。
fn sync_trace<const N: usize>(
    chunks: Vec<&'static [u8]>,
    mut push: impl FnMut(Result<i64, &'static str>) -> PushVerdict,
) -> (Vec<Result<i64, &'static str>>, EventPumpStats) {
    let mut source = ChunkSource::<SyncChunked, _, String, String, N>::new(
        SyncChunked::new(chunks),
        String::new(),
        |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
    );
    let mut out = Vec::new();
    let stats = pump_events::<ParseI64, _, _>(&mut (), &mut source, |o| {
        out.push(o);
        push(o)
    });
    (out, stats)
}

/// 异步域迹：同一块序列，AsyncLineSource → 条目 → 变换输出 → 裁决账。
async fn async_trace(
    chunks: Vec<&'static [u8]>,
    mut push: impl FnMut(Result<i64, &'static str>) -> PushVerdict,
) -> (Vec<Result<i64, &'static str>>, EventPumpStats) {
    let mut source = AsyncLineSource::<AsyncChunked, _, String, String>::new(
        AsyncChunked {
            chunks,
            at: 0,
        },
        String::new(),
        |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
        16, // 块容量 ≥ 最长块，两域读到的字节切片逐块一致
    );
    let mut out = Vec::new();
    let stats = pump_events_async::<ParseI64, _, _>(&mut (), &mut source, async |o| {
        out.push(o);
        push(o)
    })
    .await;
    (out, stats)
}

// ── 景一：全序列（含失败条目）零分歧 ──────────────────────────────────

#[test]
fn full_event_sequence_traces_match() {
    let chunks = vec![b"1\nx\n3\n".as_slice()];
    let (sync_out, sync_stats) = sync_trace::<16>(chunks.clone(), |_| PushVerdict::Delivered);
    let (async_out, async_stats) = block_on(async_trace(chunks, |_| PushVerdict::Delivered));

    // 同输入 → 同输出序列（失败也是数据：Err 被转发）→ 同裁决账。
    assert_eq!(sync_out, async_out, "T6 事件接缝：同步/异步输出序列零分歧");
    assert_eq!(sync_out, vec![Ok(1), Err("bad"), Ok(3)], "期望全序列");
    assert_eq!(sync_stats, async_stats, "T6 事件接缝：同步/异步裁决账零分歧");
    assert_eq!(sync_stats.total(), 3, "配对律：判定之和 = 拉取总数");
}

// ── 景二：跨块切割 + EOF 残留（分割语义逐块一致）──────────────────────

#[test]
fn chunked_feeds_and_eof_residue_traces_match() {
    // 一行跨两块到达；EOF 时残留半行不冲刷（同首案例：命令总以 \n 结束）。
    let chunks = vec![b"12\nab".as_slice(), b"cd34\n".as_slice()];
    let (sync_out, sync_stats) = sync_trace::<16>(chunks.clone(), |_| PushVerdict::Delivered);
    let (async_out, async_stats) = block_on(async_trace(chunks, |_| PushVerdict::Delivered));

    assert_eq!(sync_out, async_out, "T6 跨块切割：两域条目序列零分歧");
    assert_eq!(sync_out, vec![Ok(12), Err("bad")], "跨块拼接 + 失败条目");
    assert_eq!(sync_stats, async_stats);

    // EOF 残留：结尾无 \n 的半行在两域都不被冲刷 → 同只投 1 条。
    let partial = vec![b"5\n6".as_slice()];
    let (sync_out, sync_stats) = sync_trace::<16>(partial.clone(), |_| PushVerdict::Delivered);
    let (async_out, async_stats) = block_on(async_trace(partial, |_| PushVerdict::Delivered));
    assert_eq!(sync_out, async_out, "T6 EOF 残留：两域不冲刷行为零分歧");
    assert_eq!(sync_out, vec![Ok(5)], "EOF 残留半行不投递（同首案例）");
    assert_eq!(sync_stats, async_stats);
    assert_eq!(sync_stats.total(), 1);
}

// ── 景三：拆除语义（消费端断连）零分歧 ────────────────────────────────

#[test]
fn teardown_traces_match_both_domains() {
    let chunks = vec![b"1\nafter\n".as_slice()];
    // 消费端第一条后断连：泵停止拉取（"after" 永不被拉取）、未投递计 dropped。
    let mut closed = 0u32;
    let (sync_out, sync_stats) = sync_trace::<16>(chunks.clone(), |_| {
        closed += 1;
        PushVerdict::Closed
    });
    assert_eq!(sync_out.len(), 1, "同步域拆除：只拉取了 1 条");
    assert_eq!(sync_stats.delivered, 0);
    assert_eq!(sync_stats.dropped, 1, "断连时未投递的那条计 dropped");
    assert_eq!(sync_stats.total(), 1);

    let mut closed = 0u32;
    let (async_out, async_stats) = block_on(async_trace(chunks, |_| {
        closed += 1;
        PushVerdict::Closed
    }));
    assert_eq!(async_out.len(), 1, "异步域拆除：只拉取了 1 条");
    assert_eq!(async_stats, sync_stats, "T6 拆除语义：裁决账零分歧");
}
