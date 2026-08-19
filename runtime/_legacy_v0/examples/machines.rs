//! # Redis-style server — machine set
//!
//! Machine implementations for the 6 modules in the `blueprint.rs` blueprint:
//!
//! | Module | Responsibility | Input | Output | State |
//! |---|---|---|---|---|
//! | `ConnReader` | Physical socket read | `io` (IoEvent) | `raw` (conn_id, bytes) | shared connection table |
//! | `RespParser` | Incremental RESP parsing | `raw` | `cmd` (ParsedCommand) | per-connection parse buffer |
//! | `DataStore` | KV/List/Hash semantics | `cmd` | `reply` + `log` | data store |
//! | `RespEncoder` | RESP encoding (stateless pure transform, FusedInline) | `reply` | `out` (conn_id, bytes) | — |
//! | `ConnWriter` | Physical socket write | `resp` | — | shared connection table |
//! | `AofWriter` | AOF append persistence | `log` (Option<line>) | — | file handle |
//!
//! All connection dynamism lives in `State` (`HashMap<conn_id, ...>`); the topology stays static.

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
// Shared physical resource: connection table (OS-level sockets, shared across the abstraction boundary)
// ════════════════════════════════════════════════════════════════════════

/// Connection table: conn_id → TcpStream. Filled by main's accept loop;
/// shared by `ConnReader` (read) and `ConnWriter` (write).
pub type SharedTable = Arc<Mutex<ConnTable>>;

/// The process-global shared connection table: `ConnReader`/`ConnWriter` (each creates its own
/// State in init) and main's accept loop must operate on the **same** table — enforced with a
/// process-level singleton. This is physical sharing at the OS-resource layer (not expressed in the blueprint).
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
// Data types (port payloads)
// ════════════════════════════════════════════════════════════════════════

/// Raw connection bytes: `(conn_id, bytes)`; empty `bytes` = EOF/closed.
#[derive(Debug, Clone, PartialEq)]
pub struct RawBytes(pub usize, pub Vec<u8>);

/// One parsed command.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    pub conn_id: usize,
    pub name: String,          // uppercase command name
    pub args: Vec<Vec<u8>>,    // command arguments (excluding the command name)
}

/// A logical reply (protocol-independent; encoded by RespEncoder).
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
// Module 1: ConnReader — physical read (event-driven)
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
            return SingleOutput::Idle; // connection closed/unknown
        };
        let mut buf = [0u8; 8192];
        match stream.read(&mut buf) {
            Ok(0) => {
                // EOF: peer closed → remove from table, send empty bytes downstream as close marker.
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
// Module 2: RespParser — incremental RESP command parsing
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
    type State = HashMap<usize, Vec<u8>>; // conn_id → unconsumed parse buffer
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
            // EOF: discard this connection's incomplete parse buffer.
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
            None => SingleOutput::Idle, // incomplete; wait for more bytes
        }
    }
    fn cleanup(_: HashMap<usize, Vec<u8>>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

/// Attempts to parse one RESP command from the start of the buffer: `*N\r\n ($len\r\n bytes\r\n){N}`.
/// Returns `(args, consumed)`; returns None when the buffer is insufficient (buffer is retained).
fn try_parse_command(buf: &[u8]) -> Option<(Vec<Vec<u8>>, usize)> {
    let (n_line, rest) = split_crlf(buf)?;
    let n: usize = std::str::from_utf8(n_line.strip_prefix(b"*")?).ok()?.parse().ok()?;
    if n == 0 || n > 1024 {
        return None; // safety cap (prevents malicious/malformed input from exhausting memory)
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
            return None; // incomplete
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
// Debug commands (Control flow: DEBUG FLUSH / DEBUG SET / DEBUG INFO)
// ════════════════════════════════════════════════════════════════════════

/// Debug command: injected into the DataStore's **Control flow** from outside (the Debugger module).
/// Debugging is out-of-band control — it does **not** go to the AOF (does not pollute the persisted event source).
#[derive(Debug, Clone, PartialEq)]
pub enum DebugCmd {
    /// Clears the data store.
    Flush,
    /// Directly injects a key/value pair.
    Set(String, String),
    /// Returns statistics (key count).
    Info,
}

// ════════════════════════════════════════════════════════════════════════
// Module 8.5: BroadcastTee — explicit fan-out (dynamic path broadcast)
// ════════════════════════════════════════════════════════════════════════
//
// Axiom dynamic-path routing is 1-to-1: one output port can link to only one target. Fan-out is
// the **machine's responsibility** (Split/CloneSplit contract) — this machine clones and
// broadcasts one Control command to both shards (debugger → tee → data_store_0.ctrl + data_store_1.ctrl).
// Linking one output port to two targets directly in a blueprint is rejected by validate_deep
// (FanOutViaTee) — fan-out must be explicit.

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
// Module 2.5: Sharder — shard routing (complex topology: fan-out to N DataStores)
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

/// Shard routing target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardTarget {
    Zero,
    One,
    /// Global commands (FLUSHALL) must broadcast to all shards.
    Both,
}

/// Decides the shard by key hash (deterministic: the same key always goes to the same shard).
///
/// Keyless commands:
/// - `FLUSHALL` → broadcast (Both) — both shards must be cleared;
/// - `PING` / `QUIT` / `INFO` → single to shard0 (reply uniqueness: one command produces
///   exactly one reply, avoiding writing the same conn_id to the socket twice).
pub fn shard_of(cmd: &ParsedCommand) -> ShardTarget {
    match cmd.name.as_str() {
        "FLUSHALL" => ShardTarget::Both,
        "PING" | "QUIT" | "INFO" | "DEBUG" => ShardTarget::Zero,
        _ => {
            // Keyed command: args[0] is the key. FNV-1a-style hash → 2 shards.
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
                    // Broadcast: one copy to each shard (cloned).
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
// Module 3: DataStore — KV / List / Hash data semantics (Redis single-threaded data layer)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DataStorePorts {
        input type DataStoreInput {
            cmd  [Data]    => ParsedCommand,
            ctrl [Control] => DebugCmd, // debug injection (Control flow, does not change main data semantics)
        }
        output type DataStoreOutput {
            reply   [Data]    => RespValue,
            log     [Data]    => Option<Vec<u8>>,
            observe [Observe] => (usize, String),
        }
    }
}

/// Stored value: string / list / hash.
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
                // Write command → AOF log line (RESP format, replayable); read command → None.
                let log = is_write.then(|| encode_command(&cmd));
                // observe: the observer port (for tests/replay assertions).
                let summary = format!("{} => {:?}", cmd.name, kind);
                MultiOutput::YieldMulti(vec![
                    DataStoreOutput::reply(RespValue { conn_id, kind }),
                    DataStoreOutput::log(log),
                    DataStoreOutput::observe((conn_id, summary)),
                ])
            }
            // Debug injection (Control flow): out-of-band control, **not logged to AOF**.
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

/// Command dispatch table: executes and returns (reply, whether it is a write).
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

/// Encodes a command as RESP format (AOF line, replayable).
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
// Module 4: RespEncoder — RESP encoding (stateless pure transform, fusable)
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
// Module 5: ConnWriter — physical write-back (writes socket via connection table)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ConnWriterPorts {
        input type ConnWriterInput {
            resp [Data] => (usize, Vec<u8>),
        }
        output type ConnWriterOutput {
            // pure sink: no output ports
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
            // Simplification: write directly (replies are usually < MTU). Non-blocking + pending
            // write buffer + WRITABLE events are a later physicalization increment (see blueprint module docs).
            let _ = stream.write_all(&bytes);
        }
        SingleOutput::Idle
    }
    fn cleanup(_: SharedTable, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 6: AofWriter — append persistence (write-command log)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct MonitorPorts {
        input type MonitorInput {
            log [Observe] => (usize, String), // subscribes to data_store.observe
        }
        output type MonitorOutput {
            report [Observe] => (usize, String), // pass-through observe (no downstream → terminal output, for assertions)
        }
    }
}

/// Monitor state: event count + ring buffer of recent events (slow observer, bounded capacity).
pub struct MonitorState {
    pub events: u64,
    pub ring: Vec<String>,
}

/// Simulated slow-observer workload (ns/event; used by bench, default 0 = no simulation).
/// Real observation (log formatting/disk writes/aggregation) is far slower than the main path — simulated with busy-wait.
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
        // Simulate a slow observer (bench injection): the observer is slower than the main path → backlog/drop semantics kick in
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
        // Pass-through observe (the observation stream must not affect the observed module — this output only consumes, no feedback)
        SingleOutput::Yield(MonitorOutput::report((conn_id, line)))
    }
    fn cleanup(_: MonitorState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 8: Debugger — debug injection (reverse Control-flow input)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DebuggerPorts {
        input type DebuggerInput {
            cmd [Data] => DebugCmd, // externally injected (main's debug entry point)
        }
        output type DebuggerOutput {
            out [Control] => DebugCmd, // Control flow → data_store.ctrl
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
            // pure sink: no output ports
        }
    }
}

/// AOF state: lazily opened file handle (path derived from the instance name, supports per-instance shard logs).
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
                // Path derived from the instance name: aof_writer → redis_like.aof,
                // aof_writer_0/1 → redis_like_aof_writer_0.aof (one log per shard of the cluster).
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
            // Simplification: no per-line flush (batched disk writes are a physicalization increment).
        }
        SingleOutput::Idle
    }
    fn cleanup(_: AofState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}



