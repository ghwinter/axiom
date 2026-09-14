//! tokio 事件流实例：把异步事件接缝（[`AsyncEventStream`]）在 tokio 物理上兑现。
//!
//! ## 形态（§9.3 异步域对偶）
//!
//! 语义层 [`ChunkSource`](axiom_semantics::seams::event::ChunkSource) 是同步
//! `std::io::Read` 块源；本模块是它的 tokio 对偶：读取面用 `tokio::io::AsyncRead`
//! 块读（等待点挂 tokio reactor，非 `thread::sleep` 让步），分割器、队列、泵
//! 语义与同步源同构。同一接缝契约（[`AsyncEventStream`]），不同物理兑现
//! （T6 多物理实现语义等价：同步/异步同输入块序列 → 同条目序列 → 同裁决序列，
//! 交叉验证见本模块测试与 `instances/tests/async_event_crosscheck.rs`）。
//!
//! ## 与 tokio_poll_fed 的分工
//!
//! [`tokio_poll_fed`](crate::backend::async_driver::tokio_poll_fed) 从 tokio mpsc
//! 通道馈入条目级输入（单槽 `Poller::put`）；本模块从 `AsyncRead` 原始源读块、
//! 经分割器产出条目（`next_in` 队列），驱动 [`pump_events_async`] 逐个变换与
//! 投递。前者是"等待窗内的通道馈入"，后者是"外部世界（套接字/流）经块读进入
//! 因果流"——首案例 `redis_like --tcp` 的异步落点。
//!
//! ## 义务（与同步源同构）
//!
//! 配对律 / 失败归属 / 拆除语义同 [`ChunkSource`]；退化态拒绝：块容量 `N = 0`
//! 使源无法推进——构造点模态②门断言（同同步源）。成本：构造期预留一个
//! 复用读缓冲（每源一份），稳态每事件零分配（除分割器产出的条目）。
//!
//! 门控：`tokio` feature（`axiom-instances`）。安全：无 unsafe。

use axiom_semantics::seams::event::AsyncEventStream;
use std::collections::VecDeque;
use tokio::io::{AsyncRead, AsyncReadExt};

/// tokio 块源：`AsyncRead` 原始源 + 分割器 → [`AsyncEventStream`]（异步域
/// [`ChunkSource`](axiom_semantics::seams::event::ChunkSource) 对偶）。
///
/// 读缓冲容量 `N` 为运行期构造参数（构造点断言 ≥ 1，模态② 门）；跨块状态
/// （如行拼接）由 `split` 状态持有，每源一份。与同步源同构：`eof` 后不再拉取。
pub struct AsyncLineSource<R, F, SS, In> {
    reader: R,
    split: F,
    state: SS,
    queue: VecDeque<In>,
    buf: Vec<u8>,
    eof: bool,
}

impl<R, F, SS, In> AsyncLineSource<R, F, SS, In>
where
    R: AsyncRead + Unpin,
    F: FnMut(&mut SS, &[u8]) -> Vec<In>,
{
    /// 新建：异步读取面 + 分割器状态 + 块读缓冲容量 `block_cap`。
    ///
    /// 退化态拒绝（模态② 门）：`block_cap = 0` 使源无法推进（违背目的条款
    /// "源能推进"），构造点拒绝——与同步源 / `BoundedRing` 的 CAP≥1 门同门。
    pub fn new(reader: R, state: SS, split: F, block_cap: usize) -> Self {
        assert!(block_cap > 0, "AsyncLineSource 块容量必须 >= 1");
        AsyncLineSource {
            reader,
            split,
            state,
            queue: VecDeque::new(),
            buf: vec![0u8; block_cap],
            eof: false,
        }
    }

    /// 源是否已关闭（EOF 或读错误）。
    pub fn is_closed(&self) -> bool {
        self.eof
    }
}

impl<R, F, SS, In> AsyncEventStream<In> for AsyncLineSource<R, F, SS, In>
where
    R: AsyncRead + Unpin + Send,
    F: FnMut(&mut SS, &[u8]) -> Vec<In> + Send,
    SS: Send,
    In: Send,
{
    async fn next_in(&mut self) -> Option<In> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Some(item);
            }
            if self.eof {
                return None;
            }
            match self.reader.read(&mut self.buf).await {
                Ok(0) => {
                    self.eof = true;
                    return None; // EOF：残留行保留在分割状态中（不冲刷，同首案例）
                }
                Ok(n) => {
                    let items = (self.split)(&mut self.state, &self.buf[..n]);
                    self.queue.extend(items);
                }
                Err(_) => {
                    self.eof = true; // 读错误按关闭处理（同首案例）
                    return None;
                }
            }
        }
    }
}

/// 异步泵驱动：把 [`AsyncEventStream`] 的每个 `In` 经 cell `A` 变换、逐条投递——
/// 语义层 [`pump_events_async`](axiom_semantics::seams::event::pump_events_async)
/// 的直接 re-export（实例层便捷路径；义务同源）。
pub use axiom_semantics::seams::event::pump_events_async as pump_events_async;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::cell_core::PortCell;
    use axiom_semantics::seams::event::{PushVerdict, split_lines};
    use std::io::{self, Cursor};

    struct ParseI64;
    impl PortCell for ParseI64 {
        type In = String;
        type Out = Result<i64, &'static str>;
        type State = ();
        fn step(_: &mut (), line: String) -> Result<i64, &'static str> {
            line.trim().parse::<i64>().map_err(|_| "bad")
        }
    }

    /// 异步内存源：`&[u8]`（tokio 为其实现 `AsyncRead`；一次性全量读）。
    /// 交叉验证测试用同步 Cursor 源与它同内容。
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("current-thread rt")
            .block_on(f)
    }

    /// 行源全量拉取：异步块读 + 行分割 → 条目序列。
    #[test]
    fn async_line_source_yields_lines_across_chunks() {
        // 一行被切成多块到达：跨块拼接正确（与同步 ChunkSource 同语义）。
        // tokio 的 &[u8] AsyncRead 一次全量读；为模拟跨块，用一个分块读取器。
        struct Chunked {
            chunks: Vec<&'static [u8]>,
            at: usize,
            pos: usize,
        }
        impl AsyncRead for Chunked {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                out: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<io::Result<()>> {
                let this = self.get_mut();
                // 逐块填满 out：块内跨多次 poll（pos 推进），块间循环衔接；
                // 不返回 0 字节的"伪 EOF"（AsyncRead 契约：Ready + 0 字节 = EOF）。
                loop {
                    if this.at >= this.chunks.len() {
                        return std::task::Poll::Ready(Ok(())); // 实际 EOF
                    }
                    let chunk = this.chunks[this.at];
                    if this.pos >= chunk.len() {
                        this.at += 1;
                        this.pos = 0;
                        continue;
                    }
                    if out.remaining() == 0 {
                        return std::task::Poll::Ready(Ok(())); // out 已满，下次再填
                    }
                    let n = (chunk.len() - this.pos).min(out.remaining());
                    out.put_slice(&chunk[this.pos..this.pos + n]);
                    this.pos += n;
                }
            }
        }

        let reader = Chunked {
            chunks: vec![b"12\nab".as_slice(), b"cd34\n".as_slice()],
            at: 0,
            pos: 0,
        };
        let mut src = AsyncLineSource::<_, _, String, String>::new(
            reader,
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
            4,
        );
        let mut lines = Vec::new();
        block_on(async {
            while let Some(line) = src.next_in().await {
                lines.push(line);
            }
        });
        assert_eq!(lines, vec!["12", "abcd34"]);
        assert!(src.is_closed());
    }

    /// 配对律：异步泵的 delivered+dropped == 拉取总数；失败也是数据。
    #[test]
    fn async_pump_pair_law_totals_match() {
        let reader = Cursor::new(&b"1\nx\n3\n"[..]);
        let mut src = AsyncLineSource::<_, _, String, String>::new(
            reader,
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
            16,
        );
        let mut out = Vec::new();
        let stats = block_on(async {
            pump_events_async::<ParseI64, _, _>(&mut (), &mut src, async |outcome| {
                out.push(outcome);
                PushVerdict::Delivered
            })
            .await
        });
        assert_eq!(out, vec![Ok(1), Err("bad"), Ok(3)], "失败经转发由 sink 处置");
        assert_eq!(stats.delivered, 3);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.total(), 3, "配对律：判定之和 = 拉取总数");
    }

    /// 拆除语义：消费端断连 → 泵停止拉取、未投递计 dropped（同同步泵）。
    #[test]
    fn async_pump_teardown_stops_pulling() {
        let reader = Cursor::new(&b"1\nafter\n"[..]);
        let mut src = AsyncLineSource::<_, _, String, String>::new(
            reader,
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
            16,
        );
        let stats = block_on(async {
            pump_events_async::<ParseI64, _, _>(&mut (), &mut src, async |_| PushVerdict::Closed).await
        });
        assert_eq!(stats.delivered, 0);
        assert_eq!(stats.dropped, 1, "断连时未投递的那条计 dropped");
        assert_eq!(stats.total(), 1, "只拉取了 1 条（拆除语义：停止）");
    }

    /// 退化态拒绝：块容量 0 → 构造点 panic（模态② 门）。
    #[test]
    #[should_panic(expected = "块容量必须 >= 1")]
    fn zero_block_capacity_rejected_at_construction() {
        let _ = AsyncLineSource::<Cursor<&[u8]>, _, String, String>::new(
            Cursor::new(&b""[..]),
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
            0,
        );
    }
}
