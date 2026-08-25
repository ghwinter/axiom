//! 事件基座载体类（§9.3 从首案例到载体类）。std 门控。
//!
//! 事件基座回答 §9.3 的接缝问题：**外部世界（套接字事件等）如何正式成为一条因果流的
//! `in`**。本类 = 事件流（条目级输入源）＋ 泵驱动（拉取→变换→按裁决投递→计数）：
//!
//! ```text
//! 原始源 (io::Read) ─块─▶ ChunkSource（分割器＋跨块状态）─条目─▶ pump_events
//!   ─▶ A::step（A::Out，可为 Result：失败也是数据）─▶ push（投递裁决：Delivered/Closed）
//! ```
//!
//! `redis_like --tcp`（`runtime/examples/redis_like/server.rs`）是该接缝的参考实现
//! （首案例）；本类是它的泛化形态，`server.rs::handle_conn` 已改为由本类驱动。
//!
//! **概念归属**（§8.3 封闭判据）：不引入新概念——事件流是物理层输入侧的迭代器形态
//! （与 [`flow::bounded_pump`](crate::flow::bounded_pump) 使用 `IntoIterator`
//! 同属机器类）；泵驱动是 driver 的一个实例。
//!
//! **义务（A3 落位）**：
//! - 配对律：N 条事件 ↔ N 个判定（`delivered + dropped`），经 [`EventPumpStats`]
//!   机械统计（模态③，测试见证，账本行见 `obligation::LEDGER_STD_EXTRA`）。
//! - 失败归属：`A::Out` 是 [`Result`] 时，**失败也是数据**——泵不短路吞值（不做
//!   丢弃裁决），失败如何处置（转发/计数/丢弃）由 sink 的 `push` 裁决；这与
//!   `redis_like` 首案例一致（解析错误经通道转发为 `-ERR` 应答）。
//! - 拆除语义：消费端断连（[`PushVerdict::Closed`]）⟹ 泵停止拉取、不再静默延续
//!   （同 `bounded_pump` 断连语义）；未投递条数进 `dropped`（不静默丢值）。
//! - 退化态拒绝（第五轴，boundary-ontology 命题 2.7）：块容量 `N = 0` 使源无法推进，
//!   违背目的条款"源能推进"——构造点经模态②门
//!   （[`contract::assert_capacity_nonzero`](crate::contract::assert_capacity_nonzero)）
//!   编译期拒绝（同 CAP≥1 门）。
//! - 成本（D4）：事件流持有一个复用读缓冲 `[u8; N]`（构造期一次预留），稳态每事件
//!   零分配（除分割器产出的条目本身）。

use alloc::collections::VecDeque;
use std::io::Read;

use axiom::cell_core::PortCell;

/// 泵的统计账（配对律：N 条事件 ↔ N 个判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventPumpStats {
    /// 已投递到 sink 的事件数（`push` 裁决为 `Delivered`）。
    pub delivered: usize,
    /// 消费端断连时未投递的事件数（拆除：泵停止拉取，不静默延续）。
    pub dropped: usize,
}

impl EventPumpStats {
    /// 配对律：全部判定之和 = 泵拉取的事件总数。
    pub fn total(&self) -> usize {
        self.delivered + self.dropped
    }
}

/// 有界投递的单条裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushVerdict {
    /// 已投递（sink 接收）。
    Delivered,
    /// 消费端已断连（拆除）：本条未投递，泵停止拉取。
    Closed,
}

/// 事件流：产出因果流的条目级输入 `In`（分割后）。`None` = 源关闭（EOF），
/// 此后不再调用。物理层输入侧的迭代器形态——非新概念（§8.3）。
pub trait EventStream<In> {
    /// 下一个 `In`；`None` = 源关闭。
    fn next_in(&mut self) -> Option<In>;
}

/// 行分割（`redis_like` 的 `LineSplit` 语义的通用形态）：把 `&[u8]` 块按 `\n`
/// 拆为条目（去行尾、去首尾空白），未完成行保留在 `buf`（跨块拼接）。
///
/// 与首案例一致：**EOF 时不冲刷残留行**——残留只可能在后续块中完成；连接关闭时
/// 未完成的半个条目按协议语义丢弃（Redis 协议的命令总以 `\n` 结束）。
pub fn split_lines(buf: &mut String, chunk: &[u8]) -> Vec<String> {
    buf.push_str(&String::from_utf8_lossy(chunk));
    let mut lines = Vec::new();
    while let Some(idx) = buf.find('\n') {
        let line = buf[..idx].trim().to_string();
        buf.drain(..=idx);
        lines.push(line);
    }
    lines
}

/// 块源适配：`io::Read` 原始源 + 分割器 → [`EventStream`]。
///
/// 跨块状态（如行拼接的未完成行）由 `split` 状态持有，每源一份。读缓冲长度 `N`
/// 为编译期常量；**`N = 0` 是退化态**（目的条款"源能推进"被违背），构造点经模态②门
/// 编译期拒绝（同 `BoundedCarrier`/`BoundedRing` 的 CAP≥1 门）。
pub struct ChunkSource<R, F, SS, In, const N: usize> {
    reader: R,
    split: F,
    state: SS,
    queue: VecDeque<In>,
    buf: [u8; N],
    eof: bool,
}

impl<R, F, SS, In, const N: usize> ChunkSource<R, F, SS, In, N>
where
    R: Read,
    F: FnMut(&mut SS, &[u8]) -> Vec<In>,
{
    /// 新建（模态②门：`N = 0` 编译期拒绝——零容量块源是退化态）。
    pub fn new(reader: R, state: SS, split: F) -> Self {
        const { crate::contract::assert_capacity_nonzero::<N>() };
        Self {
            reader,
            split,
            state,
            queue: VecDeque::new(),
            buf: [0u8; N],
            eof: false,
        }
    }

    /// 源是否已关闭（EOF 或读错误）。
    pub fn is_closed(&self) -> bool {
        self.eof
    }
}

impl<R, F, SS, In, const N: usize> EventStream<In> for ChunkSource<R, F, SS, In, N>
where
    R: Read,
    F: FnMut(&mut SS, &[u8]) -> Vec<In>,
{
    fn next_in(&mut self) -> Option<In> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Some(item);
            }
            if self.eof {
                return None;
            }
            match self.reader.read(&mut self.buf) {
                Ok(0) => {
                    self.eof = true;
                    return None; // EOF：残留行保留在分割状态中（不冲刷，同首案例）
                }
                Ok(n) => {
                    let items = (self.split)(&mut self.state, &self.buf[..n]);
                    self.queue.extend(items);
                }
                Err(_) => {
                    self.eof = true; // 读错误按关闭处理（物理选择，同首案例 `Err(_) => break`）
                    return None;
                }
            }
        }
    }
}

/// 泵驱动：把事件流的每个 `In` 经 cell `A` 变换，再把每个 `A::Out` 经 `push`
/// 投递到 sink。
///
/// - [`PushVerdict::Delivered`] → `delivered` 计一；[`PushVerdict::Closed`]
///   （消费端断连）→ `dropped` 计一并**停止拉取**（拆除语义，同 `bounded_pump`：
///   不静默延续生产）。
/// - 失败归属：`A::Out` 为 [`Result`] 时**失败也是数据**——泵不短路吞值；失败如何
///   处置由 `push` 裁决（首案例：解析错误转发为 `-ERR` 应答）。
/// - 配对律：`delivered + dropped = 泵拉取的事件总数`（[`EventPumpStats::total`]）。
/// - 背压由 `push` 内嵌（如有界通道满时阻塞），本驱动不替代背压机制。
pub fn pump_events<A, St, Push>(
    a_state: &mut A::State,
    stream: &mut St,
    mut push: Push,
) -> EventPumpStats
where
    A: PortCell,
    St: EventStream<A::In>,
    Push: FnMut(A::Out) -> PushVerdict,
{
    let mut stats = EventPumpStats::default();
    while let Some(input) = stream.next_in() {
        let out = A::step(a_state, input);
        match push(out) {
            PushVerdict::Delivered => stats.delivered += 1,
            PushVerdict::Closed => {
                stats.dropped += 1;
                break;
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor};

    /// 测试变换单元：行 → `Result<i64, &'static str>`（失败为值）。
    pub struct ParseI64;
    impl PortCell for ParseI64 {
        type In = String;
        type Out = Result<i64, &'static str>;
        type State = ();
        fn step(_: &mut (), line: String) -> Result<i64, &'static str> {
            line.trim().parse::<i64>().map_err(|_| "bad")
        }
    }

    /// 分块读取源：按测试指定的块序列喂给 reader（模拟跨块到达；块内跨多次读）。
    pub struct Chunked<'a> {
        chunks: Vec<&'a [u8]>,
        at: usize,
        pos: usize,
    }
    impl<'a> Chunked<'a> {
        pub fn new(chunks: Vec<&'a [u8]>) -> Self {
            Self {
                chunks,
                at: 0,
                pos: 0,
            }
        }
    }
    impl<'a> Read for Chunked<'a> {
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

    #[test]
    fn pump_pair_law_totals_match() {
        // 失败也是数据：Ok 与 Err 都被转发到 push（sink 裁决），配对律成立。
        let mut source = ChunkSource::<Cursor<&[u8]>, _, String, String, 16>::new(
            Cursor::new(&b"1\nx\n3\n"[..]),
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
        );
        let mut out = Vec::new();
        let stats = pump_events::<ParseI64, _, _>(&mut (), &mut source, |outcome| {
            out.push(outcome);
            PushVerdict::Delivered
        });
        assert_eq!(out, vec![Ok(1), Err("bad"), Ok(3)], "失败经转发由 sink 处置");
        assert_eq!(stats.delivered, 3);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.total(), 3, "配对律：判定之和 = 拉取总数");
    }

    #[test]
    fn pump_teardown_stops_pulling_and_counts_dropped() {
        // 消费端在第一条后断连：泵停止拉取（"after" 永不被拉取）、未投递计 dropped。
        let mut source = ChunkSource::<Cursor<&[u8]>, _, String, String, 16>::new(
            Cursor::new(&b"1\nafter\n"[..]),
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
        );
        let stats = pump_events::<ParseI64, _, _>(&mut (), &mut source, |_outcome| {
            PushVerdict::Closed
        });
        assert_eq!(stats.delivered, 0);
        assert_eq!(stats.dropped, 1, "断连时未投递的那条计 dropped");
        assert_eq!(stats.total(), 1, "只拉取了 1 条（拆除语义：停止）");
    }

    #[test]
    fn chunk_source_carries_lines_across_chunks() {
        // 一行被切成多块到达：跨块拼接正确；EOF 后残留不冲刷（同首案例 LineSplit）。
        let mut source = ChunkSource::<Chunked<'_>, _, String, String, 4>::new(
            Chunked::new(vec![b"12\nab".as_slice(), b"cd34\n".as_slice()]),
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
        );
        let mut lines = Vec::new();
        while let Some(line) = source.next_in() {
            lines.push(line);
        }
        assert_eq!(lines, vec!["12", "abcd34"]);
        assert!(source.is_closed());
    }

    #[test]
    fn split_lines_carries_partial_line_across_calls() {
        let mut buf = String::new();
        assert_eq!(split_lines(&mut buf, b"SET a 1\nGE"), vec!["SET a 1"]);
        assert_eq!(split_lines(&mut buf, b"T a 2\n"), vec!["GET a 2"]);
        assert_eq!(buf, "");
    }
}