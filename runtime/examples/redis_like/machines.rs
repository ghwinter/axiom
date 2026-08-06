//! # Redis 风格服务器 — 机器集
//!
//! 蓝图 `blueprint.rs` 中 6 个模块的 Machine 实现：
//!
//! | 模块 | 职责 | 输入 | 输出 | 状态 |
//! |---|---|---|---|---|
//! | `ConnReader` | 物理读 socket | `io` (IoEvent) | `raw` (conn_id, bytes) | 共享连接表 |
//! | `RespParser` | 增量 RESP 解析 | `raw` | `cmd` (ParsedCommand) | 每连接解析缓冲 |
//! | `DataStore` | KV/List/Hash 语义 | `cmd` | `reply` + `log` | 数据存储 |
//! | `RespEncoder` | RESP 编码（无状态纯变换，FusedInline） | `reply` | `out` (conn_id, bytes) | — |
//! | `ConnWriter` | 物理写 socket | `resp` | — | 共享连接表 |
//! | `AofWriter` | AOF 追加持久化 | `log` (Option<line>) | — | 文件句柄 |
//!
//! 连接动态性全部落在 `State`（`HashMap<conn_id, ...>`），拓扑保持静态。

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use std::sync::OnceLock;

use axiom::declare_ports;
use axiom::machine::{CleanupError, InitError, Machine, MultiOutput, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom_runtime::IoEvent;

// ════════════════════════════════════════════════════════════════════════
// 共享物理资源：连接表（OS 层 socket，跨抽象边界共享）
// ════════════════════════════════════════════════════════════════════════

/// 连接表：conn_id → TcpStream。由 main 的 accept 循环填充，
/// `ConnReader`（读）与 `ConnWriter`（写）共享。
pub type SharedTable = Arc<Mutex<ConnTable>>;

/// 全局共享连接表：`ConnReader`/`ConnWriter`（init 各自创建 State）与
/// main 的 accept 循环必须操作**同一张**表——用进程级单例保证。
/// 这是 OS 资源层的物理共享（蓝图不表达）。
pub fn shared_table() -> SharedTable {
    static TABLE: OnceLock<SharedTable> = OnceLock::new();
    TABLE
        .get_or_init(|| Arc::new(Mutex::new(ConnTable::default())))
        .clone()
}

#[derive(Default)]
pub struct ConnTable {
    pub conns: HashMap<usize, TcpStream>,
}

// ════════════════════════════════════════════════════════════════════════
// 数据类型（端口 payload）
// ════════════════════════════════════════════════════════════════════════

/// 连接原始字节：`(conn_id, bytes)`；`bytes` 为空 = EOF/关闭。
#[derive(Debug, Clone, PartialEq)]
pub struct RawBytes(pub usize, pub Vec<u8>);

/// 一条已解析命令。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    pub conn_id: usize,
    pub name: String,          // 大写命令名
    pub args: Vec<Vec<u8>>,    // 命令参数（不含命令名）
}

/// 逻辑回复（协议无关，由 RespEncoder 编码）。
#[derive(Debug, Clone, PartialEq)]
pub struct RespValue {
    pub conn_id: usize,
    pub kind: RespKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RespKind {
    Ok,
    Err(String),
    Int(i64),
    Bulk(Option<Vec<u8>>), // None = nil
}

// ════════════════════════════════════════════════════════════════════════
// 模块 1：ConnReader — 物理读（事件驱动）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ConnReaderPorts {
        input type ConnReaderInput {
            io [Data] => IoEvent,
        }
        output type ConnReaderOutput {
            raw [Data] => RawBytes,
        }
    }
}

pub struct ConnReader;

impl Machine for ConnReader {
    type State = SharedTable;
    type Input = ConnReaderInput;
    type Output = ConnReaderOutput;
    type Ports = ConnReaderPorts;
    type ProcessOutput = SingleOutput<ConnReaderOutput>;

    fn name() -> &'static str { "conn_reader" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<SharedTable, InitError> {
        Ok(shared_table())
    }
    #[inline]
    fn process(
        state: &mut SharedTable,
        _: &MachineContext,
        input: ConnReaderInput,
    ) -> SingleOutput<ConnReaderOutput> {
        let ConnReaderInput::io(evt) = input;
        let conn_id = evt.token.0;
        let mut table = state.lock().unwrap();
        let Some(stream) = table.conns.get_mut(&conn_id) else {
            return SingleOutput::Idle; // 连接已关闭/未知
        };
        let mut buf = [0u8; 8192];
        match stream.read(&mut buf) {
            Ok(0) => {
                // EOF：对端关闭 → 从表中移除，向下游发空字节标记关闭。
                table.conns.remove(&conn_id);
                SingleOutput::Yield(ConnReaderOutput::raw(RawBytes(conn_id, Vec::new())))
            }
            Ok(n) => SingleOutput::Yield(ConnReaderOutput::raw(RawBytes(conn_id, buf[..n].to_vec()))),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => SingleOutput::Idle,
            Err(_) => {
                table.conns.remove(&conn_id);
                SingleOutput::Yield(ConnReaderOutput::raw(RawBytes(conn_id, Vec::new())))
            }
        }
    }
    fn cleanup(_: SharedTable, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 2：RespParser — 增量 RESP 命令解析
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct RespParserPorts {
        input type RespParserInput {
            raw [Data] => RawBytes,
        }
        output type RespParserOutput {
            cmd [Data] => ParsedCommand,
        }
    }
}

pub struct RespParser;

impl Machine for RespParser {
    type State = HashMap<usize, Vec<u8>>; // conn_id → 未解析完的缓冲
    type Input = RespParserInput;
    type Output = RespParserOutput;
    type Ports = RespParserPorts;
    type ProcessOutput = SingleOutput<RespParserOutput>;

    fn name() -> &'static str { "resp_parser" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<HashMap<usize, Vec<u8>>, InitError> {
        Ok(HashMap::new())
    }
    #[inline]
    fn process(
        state: &mut HashMap<usize, Vec<u8>>,
        _: &MachineContext,
        input: RespParserInput,
    ) -> SingleOutput<RespParserOutput> {
        let RespParserInput::raw(RawBytes(conn_id, bytes)) = input;
        if bytes.is_empty() {
            // EOF：丢弃该连接的未完成解析缓冲。
            state.remove(&conn_id);
            return SingleOutput::Idle;
        }
        let buf = state.entry(conn_id).or_default();
        buf.extend_from_slice(&bytes);
        match try_parse_command(buf) {
            Some((args, consumed)) => {
                buf.drain(..consumed);
                let name = String::from_utf8_lossy(&args[0]).to_ascii_uppercase();
                let cmd = ParsedCommand {
                    conn_id,
                    name,
                    args: args[1..].to_vec(),
                };
                SingleOutput::Yield(RespParserOutput::cmd(cmd))
            }
            None => SingleOutput::Idle, // 不完整，等更多字节
        }
    }
    fn cleanup(_: HashMap<usize, Vec<u8>>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

/// 尝试从缓冲开头解析一条 RESP 命令：`*N\r\n ($len\r\n bytes\r\n){N}`。
/// 返回 `(args, consumed)`；缓冲不足时返回 None（保持缓冲）。
fn try_parse_command(buf: &[u8]) -> Option<(Vec<Vec<u8>>, usize)> {
    let (n_line, rest) = split_crlf(buf)?;
    let n: usize = std::str::from_utf8(n_line.strip_prefix(b"*")?).ok()?.parse().ok()?;
    if n == 0 || n > 1024 {
        return None; // 安全上限（防恶意/畸形输入撑爆内存）
    }
    let mut rest = rest;
    let mut args = Vec::with_capacity(n);
    for _ in 0..n {
        let (len_line, after) = split_crlf(rest)?;
        let len: usize = std::str::from_utf8(len_line.strip_prefix(b"$")?).ok()?.parse().ok()?;
        if len > 512 * 1024 {
            return None;
        }
        if after.len() < len + 2 {
            return None; // 不完整
        }
        args.push(after[..len].to_vec());
        rest = &after[len + 2..];
    }
    Some((args, buf.len() - rest.len()))
}

fn split_crlf(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let pos = b.windows(2).position(|w| w == b"\r\n")?;
    Some((&b[..pos], &b[pos + 2..]))
}

// ════════════════════════════════════════════════════════════════════════
// 调试命令（Control 流：DEBUG FLUSH / DEBUG SET / DEBUG INFO）
// ════════════════════════════════════════════════════════════════════════

/// 调试命令：从外部（Debugger 模块）注入 DataStore 的 **Control 流**。
/// 调试是旁路控制——**不记 AOF**（不污染持久化事件溯源）。
#[derive(Debug, Clone, PartialEq)]
pub enum DebugCmd {
    /// 清空数据存储。
    Flush,
    /// 直接注入键值。
    Set(String, String),
    /// 返回统计（键数量）。
    Info,
}

// ════════════════════════════════════════════════════════════════════════
// 模块 8.5：BroadcastTee — 显式 fan-out（动态路径广播）
// ════════════════════════════════════════════════════════════════════════
//
// axiom 动态路径的路由是 1 对 1：一个输出端口只能链接一个目标。fan-out
// 是**机器的职责**（Split/CloneSplit 契约）——本机器把一条 Control 命令
// 克隆广播到两个分片（debugger → tee → data_store_0.ctrl + data_store_1.ctrl）。
// 蓝图里直接"一个输出端口链接两个目标"会被 validate_deep 拒绝
// （FanOutViaTee）——fan-out 必须显式。

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct BroadcastTeePorts {
        input type BroadcastTeeInput {
            cmd [Data] => DebugCmd,
        }
        output type BroadcastTeeOutput {
            out0 [Control] => DebugCmd,
            out1 [Control] => DebugCmd,
        }
    }
}

pub struct BroadcastTee;

impl Machine for BroadcastTee {
    type State = ();
    type Input = BroadcastTeeInput;
    type Output = BroadcastTeeOutput;
    type Ports = BroadcastTeePorts;
    type ProcessOutput = MultiOutput<BroadcastTeeOutput>;

    fn name() -> &'static str { "broadcast_tee" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: BroadcastTeeInput,
    ) -> MultiOutput<BroadcastTeeOutput> {
        let BroadcastTeeInput::cmd(cmd) = input;
        MultiOutput::YieldMulti(vec![
            BroadcastTeeOutput::out0(cmd.clone()),
            BroadcastTeeOutput::out1(cmd),
        ])
    }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 2.5：Sharder — 分片路由（复杂拓扑：fan-out 到 N 个 DataStore）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct SharderPorts {
        input type SharderInput {
            cmd [Data] => ParsedCommand,
        }
        output type SharderOutput {
            shard0 [Data] => ParsedCommand,
            shard1 [Data] => ParsedCommand,
        }
    }
}

/// 分片路由目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardTarget {
    Zero,
    One,
    /// 全局命令（FLUSHALL）需广播到所有分片。
    Both,
}

/// 按 key 哈希决定分片（确定性：同一 key 永远同一分片）。
///
/// 无 key 命令：
/// - `FLUSHALL` → 广播（Both）——两分片都要清空；
/// - `PING` / `QUIT` / `INFO` → 单发 shard0（回复唯一性：一条命令
///   只产生一条回复，避免同一 conn_id 被写两次 socket）。
pub fn shard_of(cmd: &ParsedCommand) -> ShardTarget {
    match cmd.name.as_str() {
        "FLUSHALL" => ShardTarget::Both,
        "PING" | "QUIT" | "INFO" | "DEBUG" => ShardTarget::Zero,
        _ => {
            // 有 key 命令：args[0] 是 key。FNV-1a 风格哈希 → 2 分片。
            let key = cmd.args.first().map(|k| k.as_slice()).unwrap_or(b"");
            let h = key.iter().fold(0u64, |h, b| {
                (h ^ *b as u64).wrapping_mul(0x100000001b3)
            });
            if h & 1 == 0 { ShardTarget::Zero } else { ShardTarget::One }
        }
    }
}

pub struct Sharder;

impl Machine for Sharder {
    type State = ();
    type Input = SharderInput;
    type Output = SharderOutput;
    type Ports = SharderPorts;
    type ProcessOutput = MultiOutput<SharderOutput>;

    fn name() -> &'static str { "sharder" }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: SharderInput,
    ) -> MultiOutput<SharderOutput> {
        match input {
            SharderInput::cmd(cmd) => match shard_of(&cmd) {
                ShardTarget::Zero => {
                    MultiOutput::YieldMulti(vec![SharderOutput::shard0(cmd)])
                }
                ShardTarget::One => {
                    MultiOutput::YieldMulti(vec![SharderOutput::shard1(cmd)])
                }
                ShardTarget::Both => {
                    // 广播：两分片各一份（克隆）。
                    MultiOutput::YieldMulti(vec![
                        SharderOutput::shard0(cmd.clone()),
                        SharderOutput::shard1(cmd),
                    ])
                }
            },
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 3：DataStore — KV / List / Hash 数据语义（Redis 单线程数据层）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DataStorePorts {
        input type DataStoreInput {
            cmd  [Data]    => ParsedCommand,
            ctrl [Control] => DebugCmd, // 调试注入（Control 流，不改变主数据语义）
        }
        output type DataStoreOutput {
            reply   [Data]    => RespValue,
            log     [Data]    => Option<Vec<u8>>,
            observe [Observe] => (usize, String),
        }
    }
}

/// 存储值：字符串 / 列表 / 哈希。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    List(VecDeque<String>),
    Hash(HashMap<String, String>),
}

pub struct DataStore;

impl Machine for DataStore {
    type State = HashMap<String, Value>;
    type Input = DataStoreInput;
    type Output = DataStoreOutput;
    type Ports = DataStorePorts;
    type ProcessOutput = MultiOutput<DataStoreOutput>;

    fn name() -> &'static str { "data_store" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<HashMap<String, Value>, InitError> {
        Ok(HashMap::new())
    }
    #[inline]
    fn process(
        state: &mut HashMap<String, Value>,
        _: &MachineContext,
        input: DataStoreInput,
    ) -> MultiOutput<DataStoreOutput> {
        match input {
            DataStoreInput::cmd(cmd) => {
                let conn_id = cmd.conn_id;
                let (kind, is_write) = execute(state, &cmd);
                // 写命令 → AOF 日志行（RESP 格式，可重放）；读命令 → None。
                let log = is_write.then(|| encode_command(&cmd));
                // observe：观察端口（供测试/重放断言）。
                let summary = format!("{} => {:?}", cmd.name, kind);
                MultiOutput::YieldMulti(vec![
                    DataStoreOutput::reply(RespValue { conn_id, kind }),
                    DataStoreOutput::log(log),
                    DataStoreOutput::observe((conn_id, summary)),
                ])
            }
            // 调试注入（Control 流）：旁路控制，**不记 AOF**。
            DataStoreInput::ctrl(cmd) => match cmd {
                DebugCmd::Flush => {
                    let n = state.len();
                    state.clear();
                    MultiOutput::YieldMulti(vec![DataStoreOutput::observe((
                        0,
                        format!("DEBUG FLUSH => cleared {n} keys"),
                    ))])
                }
                DebugCmd::Set(k, v) => {
                    state.insert(k.clone(), Value::Str(v.clone()));
                    MultiOutput::YieldMulti(vec![DataStoreOutput::observe((
                        0,
                        format!("DEBUG SET {k} => {v}"),
                    ))])
                }
                DebugCmd::Info => {
                    let kind = RespKind::Int(state.len() as i64);
                    MultiOutput::YieldMulti(vec![
                        DataStoreOutput::reply(RespValue { conn_id: 0, kind }),
                        DataStoreOutput::observe((
                            0,
                            format!("DEBUG INFO => keys={}", state.len()),
                        )),
                    ])
                }
            },
        }
    }
    fn cleanup(_: HashMap<String, Value>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

/// 命令分发表：执行并返回 (回复, 是否写)。
fn execute(state: &mut HashMap<String, Value>, cmd: &ParsedCommand) -> (RespKind, bool) {
    let a = |i: usize| cmd.args.get(i).map(|v| String::from_utf8_lossy(v).into_owned());
    match cmd.name.as_str() {
        "PING" => (RespKind::Bulk(Some(b"PONG".to_vec())), false),
        "SET" => match (a(0), a(1)) {
            (Some(k), Some(v)) => {
                state.insert(k, Value::Str(v));
                (RespKind::Ok, true)
            }
            _ => (RespKind::Err("wrong number of arguments for 'set'".into()), false),
        },
        "GET" => match a(0) {
            Some(k) => match state.get(&k) {
                Some(Value::Str(v)) => (RespKind::Bulk(Some(v.as_bytes().to_vec())), false),
                Some(_) => (RespKind::Err("WRONGTYPE operation against a key".into()), false),
                None => (RespKind::Bulk(None), false),
            },
            _ => (RespKind::Err("wrong number of arguments for 'get'".into()), false),
        },
        "DEL" => match a(0) {
            Some(k) => (RespKind::Int(state.remove(&k).is_some() as i64), true),
            _ => (RespKind::Err("wrong number of arguments for 'del'".into()), false),
        },
        "INCR" => match a(0) {
            Some(k) => {
                let cur = match state.get(&k) {
                    Some(Value::Str(s)) => s.parse::<i64>().unwrap_or(0),
                    Some(_) => {
                        return (RespKind::Err("WRONGTYPE operation against a key".into()), false)
                    }
                    None => 0,
                };
                let next = cur + 1;
                state.insert(k, Value::Str(next.to_string()));
                (RespKind::Int(next), true)
            }
            _ => (RespKind::Err("wrong number of arguments for 'incr'".into()), false),
        },
        "HSET" => match (a(0), a(1), a(2)) {
            (Some(k), Some(f), Some(v)) => {
                let h = state
                    .entry(k)
                    .or_insert_with(|| Value::Hash(HashMap::new()));
                match h {
                    Value::Hash(m) => {
                        m.insert(f, v);
                        (RespKind::Int(1), true)
                    }
                    _ => (RespKind::Err("WRONGTYPE operation against a key".into()), false),
                }
            }
            _ => (RespKind::Err("wrong number of arguments for 'hset'".into()), false),
        },
        "HGET" => match (a(0), a(1)) {
            (Some(k), Some(f)) => match state.get(&k) {
                Some(Value::Hash(m)) => match m.get(&f) {
                    Some(v) => (RespKind::Bulk(Some(v.as_bytes().to_vec())), false),
                    None => (RespKind::Bulk(None), false),
                },
                Some(_) => (RespKind::Err("WRONGTYPE operation against a key".into()), false),
                None => (RespKind::Bulk(None), false),
            },
            _ => (RespKind::Err("wrong number of arguments for 'hget'".into()), false),
        },
        "LPUSH" | "RPUSH" => match (a(0), a(1)) {
            (Some(k), Some(v)) => {
                let l = state
                    .entry(k)
                    .or_insert_with(|| Value::List(VecDeque::new()));
                match l {
                    Value::List(q) => {
                        if cmd.name == "LPUSH" {
                            q.push_front(v);
                        } else {
                            q.push_back(v);
                        }
                        (RespKind::Int(q.len() as i64), true)
                    }
                    _ => (RespKind::Err("WRONGTYPE operation against a key".into()), false),
                }
            }
            _ => (RespKind::Err("wrong number of arguments for push".into()), false),
        },
        "LPOP" | "RPOP" => match a(0) {
            Some(k) => match state.get_mut(&k) {
                Some(Value::List(q)) => {
                    let popped = if cmd.name == "LPOP" { q.pop_front() } else { q.pop_back() };
                    if q.is_empty() {
                        state.remove(&k);
                    }
                    (
                        RespKind::Bulk(popped.map(|s| s.into_bytes())),
                        true,
                    )
                }
                Some(_) => (RespKind::Err("WRONGTYPE operation against a key".into()), false),
                None => (RespKind::Bulk(None), false),
            },
            _ => (RespKind::Err("wrong number of arguments for pop".into()), false),
        },
        "EXISTS" => match a(0) {
            Some(k) => (RespKind::Int(state.contains_key(&k) as i64), false),
            _ => (RespKind::Err("wrong number of arguments for 'exists'".into()), false),
        },
        _ => (RespKind::Err(format!("unknown command '{}'", cmd.name)), false),
    }
}

/// 把命令编码为 RESP 格式（AOF 行，可重放）。
pub fn encode_command(cmd: &ParsedCommand) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(format!("*{}\r\n", cmd.args.len() + 1).as_bytes());
    push_arg(&mut out, cmd.name.as_bytes());
    for arg in &cmd.args {
        push_arg(&mut out, arg);
    }
    out
}

fn push_arg(out: &mut Vec<u8>, arg: &[u8]) {
    out.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
    out.extend_from_slice(arg);
    out.extend_from_slice(b"\r\n");
}

// ════════════════════════════════════════════════════════════════════════
// 模块 4：RespEncoder — RESP 编码（无状态纯变换，可融合）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct RespEncoderPorts {
        input type RespEncoderInput {
            reply [Data] => RespValue,
        }
        output type RespEncoderOutput {
            out [Data] => (usize, Vec<u8>),
        }
    }
}

pub struct RespEncoder;

impl Machine for RespEncoder {
    type State = ();
    type Input = RespEncoderInput;
    type Output = RespEncoderOutput;
    type Ports = RespEncoderPorts;
    type ProcessOutput = SingleOutput<RespEncoderOutput>;

    fn name() -> &'static str { "resp_encoder" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: RespEncoderInput,
    ) -> SingleOutput<RespEncoderOutput> {
        let RespEncoderInput::reply(RespValue { conn_id, kind }) = input;
        let bytes = match kind {
            RespKind::Ok => b"+OK\r\n".to_vec(),
            RespKind::Err(e) => format!("-ERR {e}\r\n").into_bytes(),
            RespKind::Int(n) => format!(":{n}\r\n").into_bytes(),
            RespKind::Bulk(None) => b"$-1\r\n".to_vec(),
            RespKind::Bulk(Some(v)) => {
                let mut b = format!("${}\r\n", v.len()).into_bytes();
                b.extend_from_slice(&v);
                b.extend_from_slice(b"\r\n");
                b
            }
        };
        SingleOutput::Yield(RespEncoderOutput::out((conn_id, bytes)))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 5：ConnWriter — 物理写回（连接表写 socket）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ConnWriterPorts {
        input type ConnWriterInput {
            resp [Data] => (usize, Vec<u8>),
        }
        output type ConnWriterOutput {
            // 纯汇：无输出端口
        }
    }
}

pub struct ConnWriter;

impl Machine for ConnWriter {
    type State = SharedTable;
    type Input = ConnWriterInput;
    type Output = ConnWriterOutput;
    type Ports = ConnWriterPorts;
    type ProcessOutput = SingleOutput<ConnWriterOutput>;

    fn name() -> &'static str { "conn_writer" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<SharedTable, InitError> {
        Ok(shared_table())
    }
    #[inline]
    fn process(
        state: &mut SharedTable,
        _: &MachineContext,
        input: ConnWriterInput,
    ) -> SingleOutput<ConnWriterOutput> {
        let ConnWriterInput::resp((conn_id, bytes)) = input;
        let mut table = state.lock().unwrap();
        if let Some(stream) = table.conns.get_mut(&conn_id) {
            // 简化：直接写（响应通常 < MTU）。非阻塞 + 待写缓冲 + WRITABLE
            // 事件是后续物理化增量（见 blueprint 模块文档）。
            let _ = stream.write_all(&bytes);
        }
        SingleOutput::Idle
    }
    fn cleanup(_: SharedTable, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 6：AofWriter — 追加持久化（写命令日志）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct MonitorPorts {
        input type MonitorInput {
            log [Observe] => (usize, String), // 订阅 data_store.observe
        }
        output type MonitorOutput {
            report [Observe] => (usize, String), // 透传观察（无下游 → 终端输出，供断言）
        }
    }
}

/// Monitor 状态：事件计数 + 最近事件环形缓冲（低速观测，容量封顶）。
pub struct MonitorState {
    pub events: u64,
    pub ring: Vec<String>,
}

/// 模拟低速观测工作负载（ns/事件；bench 用，默认 0 = 不做模拟）。
/// 真实观测（日志格式化/磁盘写入/聚合）远慢于主路径——用忙等待模拟。
pub static MONITOR_WORK_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub struct Monitor;

impl Machine for Monitor {
    type State = MonitorState;
    type Input = MonitorInput;
    type Output = MonitorOutput;
    type Ports = MonitorPorts;
    type ProcessOutput = SingleOutput<MonitorOutput>;

    fn name() -> &'static str { "monitor" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<MonitorState, InitError> {
        Ok(MonitorState {
            events: 0,
            ring: Vec::new(),
        })
    }
    #[inline]
    fn process(
        state: &mut MonitorState,
        _: &MachineContext,
        input: MonitorInput,
    ) -> SingleOutput<MonitorOutput> {
        let MonitorInput::log((conn_id, line)) = input;
        // 模拟低速观测（bench 注入）：观测模块比主路径慢 → 积压/丢弃语义生效
        let work_ns = MONITOR_WORK_NS.load(std::sync::atomic::Ordering::Relaxed);
        if work_ns > 0 {
            let start = std::time::Instant::now();
            while start.elapsed().as_nanos() < work_ns as u128 {
                std::hint::spin_loop();
            }
        }
        state.events += 1;
        state.ring.push(line.clone());
        if state.ring.len() > 64 {
            state.ring.remove(0);
        }
        // 透传观察（观测流不得影响被观测模块——本输出只消费不反作用）
        SingleOutput::Yield(MonitorOutput::report((conn_id, line)))
    }
    fn cleanup(_: MonitorState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 8：Debugger — 调试注入（Control 流反向输入）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DebuggerPorts {
        input type DebuggerInput {
            cmd [Data] => DebugCmd, // 外部注入（main 的调试入口）
        }
        output type DebuggerOutput {
            out [Control] => DebugCmd, // Control 流 → data_store.ctrl
        }
    }
}

pub struct Debugger;

impl Machine for Debugger {
    type State = ();
    type Input = DebuggerInput;
    type Output = DebuggerOutput;
    type Ports = DebuggerPorts;
    type ProcessOutput = SingleOutput<DebuggerOutput>;

    fn name() -> &'static str { "debugger" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: DebuggerInput,
    ) -> SingleOutput<DebuggerOutput> {
        let DebuggerInput::cmd(cmd) = input;
        SingleOutput::Yield(DebuggerOutput::out(cmd))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AofWriterPorts {
        input type AofWriterInput {
            log [Data] => Option<Vec<u8>>,
        }
        output type AofWriterOutput {
            // 纯汇：无输出端口
        }
    }
}

/// AOF 状态：惰性打开的文件句柄（路径由实例名派生，支持多实例分片日志）。
pub struct AofState {
    pub file: Option<std::fs::File>,
}

pub struct AofWriter;

impl Machine for AofWriter {
    type State = AofState;
    type Input = AofWriterInput;
    type Output = AofWriterOutput;
    type Ports = AofWriterPorts;
    type ProcessOutput = SingleOutput<AofWriterOutput>;

    fn name() -> &'static str { "aof_writer" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<AofState, InitError> {
        Ok(AofState { file: None })
    }
    #[inline]
    fn process(
        state: &mut AofState,
        ctx: &MachineContext,
        input: AofWriterInput,
    ) -> SingleOutput<AofWriterOutput> {
        let AofWriterInput::log(line) = input;
        if let Some(line) = line {
            let f = state.file.get_or_insert_with(|| {
                // 实例名派生路径：aof_writer → redis_like.aof，
                // aof_writer_0/1 → redis_like_aof_writer_0.aof（分片集群各自日志）。
                let path = if ctx.name() == "aof_writer" {
                    "redis_like.aof".to_string()
                } else {
                    format!("redis_like_{}.aof", ctx.name())
                };
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .expect("open AOF")
            });
            let _ = f.write_all(&line);
            // 简化：不逐条 flush（批量落盘是物理化增量）。
        }
        SingleOutput::Idle
    }
    fn cleanup(_: AofState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}



