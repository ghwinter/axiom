//! # MMO core subgraph — machine set
//!
//! Machine implementations for the 7 modules in the `blueprint.rs` blueprint:
//!
//! | Module | Responsibility | Input | Output | State |
//! |---|---|---|---|---|
//! | `ConnGateway` | Physical socket read | `io` (IoEvent) | `raw` (conn_id, bytes) | shared connection table |
//! | `ProtocolParser` | Line protocol parsing (LOGIN/MOVE/SAY/LOGOUT) | `raw` | `msg` (ClientMsg) | per-connection buffer |
//! | `SessionManager` | Session lifecycle + heartbeat timeout | `msg` + `tick`(clock) | `world`(WorldEvt) + `view`(error) | session table + now |
//! | `WorldShard` | World state (player position/online), event sourcing | `evt` + `replay` | `world`(WorldUpdate) + `log` | online players |
//! | `PerPlayerView` | View projection (world event → N player views) | `world` + `notice` | `view` (conn_id, bytes) | — |
//! | `BroadcastWriter` | Physical write-back | `view` | — | shared connection table |
//! | `EventLog` | World event log (append, replayable) | `log` | — | file handle |

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, OnceLock};

use axiom::declare_ports;
use axiom::machine::{CleanupError, InitError, Machine, MultiOutput, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom_runtime::IoEvent;

// ════════════════════════════════════════════════════════════════════════
// Shared physical resource: connection table (same pattern as redis_like)
// ════════════════════════════════════════════════════════════════════════

pub type SharedTable = Arc<Mutex<ConnTable>>;

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

/// Raw connection bytes: `(conn_id, bytes)`; empty bytes = EOF/close.
#[derive(Debug, Clone, PartialEq)]
pub struct RawBytes(pub usize, pub Vec<u8>);

// ════════════════════════════════════════════════════════════════════════
// Data types (protocol / world semantics)
// ════════════════════════════════════════════════════════════════════════

/// Player input message (line protocol: LOGIN name / MOVE x y / SAY text / LOGOUT).
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Login(String),
    Move(f32, f32),
    Say(String),
    Logout,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientMsg {
    pub conn_id: usize,
    pub msg: Msg,
}

/// World event (session layer → world layer; Join/Leave carry conn_id for projection routing).
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEvt {
    Join(usize, String),
    Move(String, f32, f32),
    Say(String, String),
    Leave(usize, String),
}

/// World update: online player snapshot + the current event.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldUpdate {
    pub players: Vec<(usize, String, f32, f32)>, // (conn_id, name, x, y)
    pub evt: Option<WorldEvt>,
}

// ════════════════════════════════════════════════════════════════════════
// Module 1: ConnGateway — physical read (reuses the redis_like ConnReader pattern)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ConnGatewayPorts {
        input type ConnGatewayInput {
            io [Data] => IoEvent,
        }
        output type ConnGatewayOutput {
            raw [Data] => RawBytes,
        }
    }
}

pub struct ConnGateway;

impl Machine for ConnGateway {
    type State = SharedTable;
    type Input = ConnGatewayInput;
    type Output = ConnGatewayOutput;
    type Ports = ConnGatewayPorts;
    type ProcessOutput = SingleOutput<ConnGatewayOutput>;

    fn name() -> &'static str { "conn_gateway" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<SharedTable, InitError> {
        Ok(shared_table())
    }
    #[inline]
    fn process(
        state: &mut SharedTable,
        _: &MachineContext,
        input: ConnGatewayInput,
    ) -> SingleOutput<ConnGatewayOutput> {
        let ConnGatewayInput::io(evt) = input;
        let conn_id = evt.token.0;
        let mut table = state.lock().unwrap();
        let Some(stream) = table.conns.get_mut(&conn_id) else {
            return SingleOutput::Idle;
        };
        let mut buf = [0u8; 8192];
        match stream.read(&mut buf) {
            Ok(0) => {
                table.conns.remove(&conn_id);
                SingleOutput::Yield(ConnGatewayOutput::raw(RawBytes(conn_id, Vec::new())))
            }
            Ok(n) => SingleOutput::Yield(ConnGatewayOutput::raw(RawBytes(
                conn_id,
                buf[..n].to_vec(),
            ))),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => SingleOutput::Idle,
            Err(_) => {
                table.conns.remove(&conn_id);
                SingleOutput::Yield(ConnGatewayOutput::raw(RawBytes(conn_id, Vec::new())))
            }
        }
    }
    fn cleanup(_: SharedTable, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 2: ProtocolParser — line protocol parsing (incremental)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ProtocolParserPorts {
        input type ProtocolParserInput {
            raw [Data] => RawBytes,
        }
        output type ProtocolParserOutput {
            msg [Data] => ClientMsg,
        }
    }
}

pub struct ProtocolParser;

impl Machine for ProtocolParser {
    type State = HashMap<usize, Vec<u8>>;
    type Input = ProtocolParserInput;
    type Output = ProtocolParserOutput;
    type Ports = ProtocolParserPorts;
    type ProcessOutput = SingleOutput<ProtocolParserOutput>;

    fn name() -> &'static str { "protocol_parser" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<HashMap<usize, Vec<u8>>, InitError> {
        Ok(HashMap::new())
    }
    #[inline]
    fn process(
        state: &mut HashMap<usize, Vec<u8>>,
        _: &MachineContext,
        input: ProtocolParserInput,
    ) -> SingleOutput<ProtocolParserOutput> {
        let ProtocolParserInput::raw(RawBytes(conn_id, bytes)) = input;
        if bytes.is_empty() {
            state.remove(&conn_id);
            return SingleOutput::Idle;
        }
        let buf = state.entry(conn_id).or_default();
        buf.extend_from_slice(&bytes);
        // find the first complete line (ends with \n)
        let Some(nl) = buf.iter().position(|&b| b == b'\n') else {
            return SingleOutput::Idle;
        };
        let line = buf[..nl].to_vec();
        buf.drain(..nl + 1);
        match parse_line(&line) {
            Some(msg) => SingleOutput::Yield(ProtocolParserOutput::msg(ClientMsg { conn_id, msg })),
            None => SingleOutput::Idle, // unparseable line: discard it
        }
    }
    fn cleanup(_: HashMap<usize, Vec<u8>>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

fn parse_line(line: &[u8]) -> Option<Msg> {
    let text = std::str::from_utf8(line).ok()?.trim();
    let mut it = text.splitn(2, ' ');
    let cmd = it.next()?.to_ascii_uppercase();
    let rest = it.next().unwrap_or("");
    match cmd.as_str() {
        "LOGIN" if !rest.is_empty() => Some(Msg::Login(rest.trim().to_string())),
        "MOVE" => {
            let mut p = rest.split_whitespace();
            let x: f32 = p.next()?.parse().ok()?;
            let y: f32 = p.next()?.parse().ok()?;
            Some(Msg::Move(x, y))
        }
        "SAY" if !rest.is_empty() => Some(Msg::Say(rest.trim().to_string())),
        "LOGOUT" => Some(Msg::Logout),
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════════════
// Module 3: SessionManager — session lifecycle + heartbeat timeout
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct SessionMgrPorts {
        input type SessionMgrInput {
            msg  [Data] => ClientMsg,
            tick [Data] => u64, // clock: millisecond timestamp (injected by main every 100ms)
        }
        output type SessionMgrOutput {
            world [Data] => Option<WorldEvt>,
            view  [Data] => Option<(usize, String)>,
        }
    }
}

#[derive(Debug)]
pub struct Session {
    name: String,
    last_seen: u64,
}

#[derive(Debug)]
pub struct SessState {
    pub sessions: HashMap<usize, Session>,
    pub now: u64,
}

/// Heartbeat timeout threshold (milliseconds; test-friendly: 10s).
pub const HEARTBEAT_TIMEOUT_MS: u64 = 10_000;

pub struct SessionManager;

impl Machine for SessionManager {
    type State = SessState;
    type Input = SessionMgrInput;
    type Output = SessionMgrOutput;
    type Ports = SessionMgrPorts;
    type ProcessOutput = MultiOutput<SessionMgrOutput>;

    fn name() -> &'static str { "session_mgr" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<SessState, InitError> {
        Ok(SessState {
            sessions: HashMap::new(),
            now: 0,
        })
    }
    #[inline]
    fn process(
        state: &mut SessState,
        _: &MachineContext,
        input: SessionMgrInput,
    ) -> MultiOutput<SessionMgrOutput> {
        match input {
            SessionMgrInput::msg(ClientMsg { conn_id, msg }) => {
                let now = state.now;
                let mut out = Vec::with_capacity(2);
                match msg {
                    Msg::Login(name) => {
                        // if already logged in, kick the old session first (same-name takeover)
                        if let Some(old) = state.sessions.remove(&conn_id) {
                            out.push(SessionMgrOutput::world(Some(WorldEvt::Leave(
                                conn_id, old.name,
                            ))));
                        }
                        state.sessions.insert(
                            conn_id,
                            Session { name: name.clone(), last_seen: now },
                        );
                        out.push(SessionMgrOutput::world(Some(WorldEvt::Join(conn_id, name))));
                    }
                    Msg::Move(x, y) => match state.sessions.get_mut(&conn_id) {
                        Some(s) => {
                            s.last_seen = now;
                            out.push(SessionMgrOutput::world(Some(WorldEvt::Move(
                                s.name.clone(), x, y,
                            ))));
                        }
                        None => out.push(SessionMgrOutput::view(Some((
                            conn_id,
                            "ERR not logged in".into(),
                        )))),
                    },
                    Msg::Say(text) => match state.sessions.get_mut(&conn_id) {
                        Some(s) => {
                            s.last_seen = now;
                            out.push(SessionMgrOutput::world(Some(WorldEvt::Say(
                                s.name.clone(), text,
                            ))));
                        }
                        None => out.push(SessionMgrOutput::view(Some((
                            conn_id,
                            "ERR not logged in".into(),
                        )))),
                    },
                    Msg::Logout => match state.sessions.remove(&conn_id) {
                        Some(s) => out.push(SessionMgrOutput::world(Some(WorldEvt::Leave(
                            conn_id, s.name,
                        )))),
                        None => out.push(SessionMgrOutput::view(Some((
                            conn_id,
                            "ERR not logged in".into(),
                        )))),
                    },
                }
                MultiOutput::YieldMulti(out)
            }
            SessionMgrInput::tick(ts) => {
                state.now = ts;
                // heartbeat timeout: kick all timed-out sessions at once (multiple Leaves)
                let stale: Vec<(usize, String)> = state
                    .sessions
                    .iter()
                    .filter(|(_, s)| ts.saturating_sub(s.last_seen) > HEARTBEAT_TIMEOUT_MS)
                    .map(|(id, s)| (*id, s.name.clone()))
                    .collect();
                let out: Vec<SessionMgrOutput> = stale
                    .into_iter()
                    .map(|(id, name)| {
                        state.sessions.remove(&id);
                        SessionMgrOutput::world(Some(WorldEvt::Leave(id, name)))
                    })
                    .collect();
                MultiOutput::YieldMulti(out)
            }
        }
    }
    fn cleanup(_: SessState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 4: WorldShard — world state (player position/online), event sourcing
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct WorldShardPorts {
        input type WorldShardInput {
            evt    [Data] => Option<WorldEvt>,
            replay [Data] => String, // event log line (restore on restart)
        }
        output type WorldShardOutput {
            world   [Data]    => WorldUpdate,
            log     [Data]    => Option<String>,
            observe [Observe] => String, // world snapshot text (no downstream → terminal output, for assertions)
        }
    }
}

pub struct WorldShard;

impl Machine for WorldShard {
    // conn_id → name；name → (x, y)
    type State = (HashMap<usize, String>, HashMap<String, (f32, f32)>);
    type Input = WorldShardInput;
    type Output = WorldShardOutput;
    type Ports = WorldShardPorts;
    type ProcessOutput = MultiOutput<WorldShardOutput>;

    fn name() -> &'static str { "world_shard" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<Self::State, InitError> {
        Ok((HashMap::new(), HashMap::new()))
    }
    #[inline]
    fn process(
        state: &mut Self::State,
        _: &MachineContext,
        input: WorldShardInput,
    ) -> MultiOutput<WorldShardOutput> {
        match input {
            WorldShardInput::evt(Some(evt)) => {
                let log_line = apply_event(state, &evt);
                let update = snapshot(state, Some(evt));
                MultiOutput::YieldMulti(vec![
                    WorldShardOutput::world(update.clone()),
                    WorldShardOutput::log(log_line),
                    WorldShardOutput::observe(observe_text(&update)),
                ])
            }
            WorldShardInput::evt(None) => {
                let update = snapshot(state, None);
                MultiOutput::YieldMulti(vec![
                    WorldShardOutput::world(update.clone()),
                    WorldShardOutput::log(None),
                    WorldShardOutput::observe(observe_text(&update)),
                ])
            }
            WorldShardInput::replay(line) => {
                // event-sourcing replay: parse the log line → apply to world state (without re-logging)
                if let Some(evt) = parse_log_line(&line) {
                    let _ = apply_event(state, &evt);
                }
                let update = snapshot(state, None);
                MultiOutput::YieldMulti(vec![
                    WorldShardOutput::world(update.clone()),
                    WorldShardOutput::log(None),
                    WorldShardOutput::observe(observe_text(&update)),
                ])
            }
        }
    }
    fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

/// Apply a world event and return the event log line (None = no recordable event).
fn apply_event(
    state: &mut (HashMap<usize, String>, HashMap<String, (f32, f32)>),
    evt: &WorldEvt,
) -> Option<String> {
    let (by_conn, by_name) = state;
    match evt {
        WorldEvt::Join(conn_id, name) => {
            by_conn.insert(*conn_id, name.clone());
            by_name.insert(name.clone(), (0.0, 0.0));
            Some(format!("JOIN {conn_id} {name}"))
        }
        WorldEvt::Move(name, x, y) => {
            if by_name.contains_key(name) {
                by_name.insert(name.clone(), (*x, *y));
                Some(format!("MOVE {name} {x} {y}"))
            } else {
                None
            }
        }
        WorldEvt::Say(name, text) => {
            if by_name.contains_key(name) {
                Some(format!("SAY {name} {text}"))
            } else {
                None
            }
        }
        WorldEvt::Leave(conn_id, name) => {
            by_conn.remove(conn_id);
            by_name.remove(name);
            Some(format!("LEAVE {conn_id} {name}"))
        }
    }
}

/// Observation text: `event: ... | players=[(conn_id, name, x, y), ...]`
/// (for deterministic assertions: event + world snapshot).
fn observe_text(update: &WorldUpdate) -> String {
    let evt_text = match &update.evt {
        Some(WorldEvt::Join(_, n)) => format!("{n} joined"),
        Some(WorldEvt::Move(n, x, y)) => format!("{n} moved to ({x},{y})"),
        Some(WorldEvt::Say(n, t)) => format!("{n} says: {t}"),
        Some(WorldEvt::Leave(_, n)) => format!("{n} left"),
        None => "(none)".to_string(),
    };
    format!("event: {evt_text} | players={:?}", update.players)
}

fn snapshot(
    state: &(HashMap<usize, String>, HashMap<String, (f32, f32)>),
    evt: Option<WorldEvt>,
) -> WorldUpdate {
    let (by_conn, by_name) = state;
    let players: Vec<(usize, String, f32, f32)> = by_conn
        .iter()
        .filter_map(|(id, name)| {
            by_name.get(name).map(|(x, y)| (*id, name.clone(), *x, *y))
        })
        .collect();
    WorldUpdate { players, evt }
}

fn parse_log_line(line: &str) -> Option<WorldEvt> {
    let mut it = line.split_whitespace();
    match it.next()? {
        "JOIN" => {
            let conn: usize = it.next()?.parse().ok()?;
            let name = it.collect::<Vec<_>>().join(" ");
            Some(WorldEvt::Join(conn, name))
        }
        "MOVE" => {
            let name = it.next()?.to_string();
            let x: f32 = it.next()?.parse().ok()?;
            let y: f32 = it.next()?.parse().ok()?;
            Some(WorldEvt::Move(name, x, y))
        }
        "SAY" => {
            let name = it.next()?.to_string();
            let text = it.collect::<Vec<_>>().join(" ");
            Some(WorldEvt::Say(name, text))
        }
        "LEAVE" => {
            let conn: usize = it.next()?.parse().ok()?;
            let name = it.collect::<Vec<_>>().join(" ");
            Some(WorldEvt::Leave(conn, name))
        }
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════════════
// Module 5: PerPlayerView — view projection (world event → N player views)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PerPlayerViewPorts {
        input type PerPlayerViewInput {
            world  [Data] => WorldUpdate,
            notice [Data] => Option<(usize, String)>,
        }
        output type PerPlayerViewOutput {
            view [Data] => (usize, Vec<u8>),
        }
    }
}

pub struct PerPlayerView;

impl Machine for PerPlayerView {
    type State = ();
    type Input = PerPlayerViewInput;
    type Output = PerPlayerViewOutput;
    type Ports = PerPlayerViewPorts;
    type ProcessOutput = MultiOutput<PerPlayerViewOutput>;

    fn name() -> &'static str { "per_player_view" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: PerPlayerViewInput,
    ) -> MultiOutput<PerPlayerViewOutput> {
        match input {
            PerPlayerViewInput::world(update) => {
                // projection: every online player receives a copy of the world view text
                let online: Vec<String> = update
                    .players
                    .iter()
                    .map(|(_, n, x, y)| format!("{n}@({x},{y})"))
                    .collect();
                let evt_text = match &update.evt {
                    Some(WorldEvt::Join(_, n)) => format!("event: {n} joined"),
                    Some(WorldEvt::Move(n, x, y)) => format!("event: {n} moved to ({x},{y})"),
                    Some(WorldEvt::Say(n, t)) => format!("event: {n} says: {t}"),
                    Some(WorldEvt::Leave(_, n)) => format!("event: {n} left"),
                    None => "event: (none)".to_string(),
                };
                let text = format!("{evt_text} | online: [{}]", online.join(", "));
                let views: Vec<PerPlayerViewOutput> = update
                    .players
                    .iter()
                    .map(|(conn_id, _, _, _)| {
                        PerPlayerViewOutput::view((*conn_id, text.clone().into_bytes()))
                    })
                    .collect();
                MultiOutput::YieldMulti(views)
            }
            PerPlayerViewInput::notice(Some((conn_id, msg))) => MultiOutput::YieldMulti(vec![
                PerPlayerViewOutput::view((conn_id, msg.into_bytes())),
            ]),
            PerPlayerViewInput::notice(None) => MultiOutput::Idle,
        }
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 6: BroadcastWriter — physical write-back (reuses the redis_like ConnWriter pattern)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct BroadcastWriterPorts {
        input type BroadcastWriterInput {
            view [Data] => (usize, Vec<u8>),
        }
        output type BroadcastWriterOutput {
            // pure sink: no output port
        }
    }
}

pub struct BroadcastWriter;

impl Machine for BroadcastWriter {
    type State = SharedTable;
    type Input = BroadcastWriterInput;
    type Output = BroadcastWriterOutput;
    type Ports = BroadcastWriterPorts;
    type ProcessOutput = SingleOutput<BroadcastWriterOutput>;

    fn name() -> &'static str { "broadcast_writer" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<SharedTable, InitError> {
        Ok(shared_table())
    }
    #[inline]
    fn process(
        state: &mut SharedTable,
        _: &MachineContext,
        input: BroadcastWriterInput,
    ) -> SingleOutput<BroadcastWriterOutput> {
        let BroadcastWriterInput::view((conn_id, bytes)) = input;
        let mut table = state.lock().unwrap();
        if let Some(stream) = table.conns.get_mut(&conn_id) {
            let _ = stream.write_all(&bytes);
        }
        SingleOutput::Idle
    }
    fn cleanup(_: SharedTable, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 7: EventLog — world event log (event sourcing, replayable to rebuild)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct EventLogPorts {
        input type EventLogInput {
            log [Data] => Option<String>,
        }
        output type EventLogOutput {
            // pure sink: no output port
        }
    }
}

pub struct LogState {
    pub file: Option<std::fs::File>,
}

pub struct EventLog;

impl Machine for EventLog {
    type State = LogState;
    type Input = EventLogInput;
    type Output = EventLogOutput;
    type Ports = EventLogPorts;
    type ProcessOutput = SingleOutput<EventLogOutput>;

    fn name() -> &'static str { "event_log" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<LogState, InitError> {
        Ok(LogState { file: None })
    }
    #[inline]
    fn process(
        state: &mut LogState,
        _: &MachineContext,
        input: EventLogInput,
    ) -> SingleOutput<EventLogOutput> {
        let EventLogInput::log(line) = input;
        if let Some(line) = line {
            let f = state.file.get_or_insert_with(|| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("world_events.log")
                    .expect("open event log")
            });
            let _ = writeln!(f, "{line}");
        }
        SingleOutput::Idle
    }
    fn cleanup(_: LogState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}



