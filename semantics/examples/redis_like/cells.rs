//! miniredis 细胞库 —— 以 cell_core 端口体表达的 KV 服务器管线（redis_like 硬化版）。
//!
//! 管线（全部为 [`PortCell`]，组合封闭、失败为值）：
//!
//! ```text
//!           ┌──────────┐   Vec<String>   ┌──────────┐  Result<Cmd,_>  ┌────────────┐
//!  bytes ──▶│ LineSplit │───────────────▶│ CmdParse │───────────────▶│  DataStore │
//!           │ (有状态   │                 │ (失败为值)│  短路(TryChain) │ (有状态+资源边界)│
//!           │  跨块缓冲)│                 └──────────┘                 └─────┬──────┘
//!           └──────────┘                                                   ▼
//!                                                 Result<(Reply,AOF行), Error>
//!                                                       │ demux/codec
//!                                                       ▼
//!                                                   StoreDemux → RESP 字符串
//! ```
//!
//! 错误模型：`Error = Parse(ParseErr) | Store(StoreErr)` 单枚举（`TryChain` 共享同一个
//! `E` 的前置），`CmdParse` / `DataStore` 的 `Out = Result<.., Error>` 使失败为值、
//! 短路为组合（概念 1/3，§7.5 全函数落点）。资源边界（键数/值上限）是 `StoreErr`
//! 的类型化值，不静默吞并。
//!
//! `ReadOnlyProxy`：演示型位的 ∃ 换装（`SlotDrive` 安装 DataStore 后换入只读代理）。

use std::collections::HashMap;

use axiom::cell_core::PortCell;

/// 命令：解析后的指令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Get(String),
    Set(String, i64),
    Del(String),
    Incr(String),
}

/// 解析错误（类型化；取代旧的 `Cmd::Protocol` 值黑客）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErr {
    /// `GET` 缺键。
    MissingKey,
    /// `SET` 缺键或值。
    MissingValue,
    /// `SET` 值非法（附原文）。
    BadValue(String),
    /// 未知命令（附原名）。
    Unknown(String),
    /// 空行。
    EmptyLine,
}

/// 存储错误（资源边界与一致性，类型化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreErr {
    /// 键数量超过 `max_keys`。
    MaxKeys,
    /// 值超过 `max_value`（附原值）。
    ValueTooLarge(i64),
    /// 只读代理拒绝写命令。
    ReadOnly,
}

/// 统一错误：`TryChain<CmdParse, DataStore>` 共享同一个 `E` 的类型化单层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Parse(ParseErr),
    Store(StoreErr),
}

impl core::fmt::Display for ParseErr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ParseErr::MissingKey => write!(f, "GET requires a key"),
            ParseErr::MissingValue => write!(f, "SET requires key and value"),
            ParseErr::BadValue(v) => write!(f, "SET value must be an integer, got '{v}'"),
            ParseErr::Unknown(c) => write!(f, "unknown command '{c}'"),
            ParseErr::EmptyLine => write!(f, "empty command"),
        }
    }
}

impl core::fmt::Display for StoreErr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StoreErr::MaxKeys => write!(f, "max keys reached"),
            StoreErr::ValueTooLarge(v) => write!(f, "value too large: {v}"),
            StoreErr::ReadOnly => write!(f, "store is read-only"),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "{e}"),
            Error::Store(e) => write!(f, "{e}"),
        }
    }
}

/// 存储操作结果（RESP 风格）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Int(i64),
    Ok,
    Nil,
}

/// 服务器配置（资源边界/健壮性）：命令/存储层的可调参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 键数量上限（写满即拒绝新增键 → 资源边界）。
    pub max_keys: usize,
    /// 值上限（超过即拒绝 → 值边界）。
    pub max_value: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config { max_keys: 10_000, max_value: 1_000_000 }
    }
}

/// 存储状态别名（有状态容器：map + AOF 日志 + 配置）。
pub type StoreState = (HashMap<String, i64>, Vec<String>, Config);

/// 新建存储状态（AOF 日志从空开始）。
pub fn new_store(cfg: Config) -> StoreState {
    (HashMap::new(), Vec::new(), cfg)
}

// ═══════════════════════════════════════════════════════════════════
// LineSplit —— 字节流 → 命令行（有状态缓冲）
// ═══════════════════════════════════════════════════════════════════

/// 把字节流按 `\n` 拆成命令行。State = 未完成行的缓冲。
pub struct LineSplit;
impl PortCell for LineSplit {
    type In = String;
    type Out = Vec<String>;
    type State = String;
    #[inline(always)]
    fn step(buf: &mut String, chunk: String) -> Vec<String> {
        buf.push_str(&chunk);
        let mut lines = Vec::new();
        while let Some(idx) = buf.find('\n') {
            let line = buf[..idx].trim().to_string();
            buf.drain(..=idx);
            lines.push(line);
        }
        lines
    }
}

// ═══════════════════════════════════════════════════════════════════
// CmdParse —— 命令行 → Result<命令, Error>（失败为值）
// ═══════════════════════════════════════════════════════════════════

/// 把一行解析为命令（`GET key` / `SET key val` / `DEL key` / `INCR key`）。
/// 失败是 `Err(Error::Parse(..))` 值——不静默吞成零值/占位。
pub struct CmdParse;
impl PortCell for CmdParse {
    type In = String;
    type Out = Result<Cmd, Error>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), line: String) -> Result<Cmd, Error> {
        let mut it = line.split_whitespace();
        match (it.next(), it.next(), it.next()) {
            (Some("GET"), Some(k), _) => Ok(Cmd::Get(k.to_string())),
            (Some("GET"), _, _) => Err(Error::Parse(ParseErr::MissingKey)),
            (Some("SET"), Some(k), Some(v)) => match v.parse::<i64>() {
                Ok(n) => Ok(Cmd::Set(k.to_string(), n)),
                Err(_) => Err(Error::Parse(ParseErr::BadValue(v.to_string()))),
            },
            (Some("SET"), _, _) => Err(Error::Parse(ParseErr::MissingValue)),
            (Some("DEL"), Some(k), _) => Ok(Cmd::Del(k.to_string())),
            (Some("DEL"), _, _) => Err(Error::Parse(ParseErr::MissingKey)),
            (Some("INCR"), Some(k), _) => Ok(Cmd::Incr(k.to_string())),
            (Some("INCR"), _, _) => Err(Error::Parse(ParseErr::MissingKey)),
            (Some(other), ..) => Err(Error::Parse(ParseErr::Unknown(other.to_string()))),
            _ => Err(Error::Parse(ParseErr::EmptyLine)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// DataStore —— KV 存储（有状态，失败为值）
// ═══════════════════════════════════════════════════════════════════

/// KV 存储（有状态，含资源边界配置）。`Out = Result<(回复, 可选AOF行), Error>`——
/// 资源越界是类型化错误值（MaxKeys / ValueTooLarge），不是 RESULT 里的 `Reply::Err`。
pub struct DataStore;
impl PortCell for DataStore {
    type In = Cmd;
    type Out = Result<(Reply, Option<String>), Error>;
    type State = StoreState;
    #[inline(always)]
    fn step((map, log, cfg): &mut StoreState, cmd: Cmd) -> Result<(Reply, Option<String>), Error> {
        match cmd {
            Cmd::Get(k) => {
                let r = map.get(&k).copied().map(Reply::Int).unwrap_or(Reply::Nil);
                Ok((r, None))
            }
            Cmd::Set(k, v) => {
                if v > cfg.max_value {
                    return Err(Error::Store(StoreErr::ValueTooLarge(v)));
                }
                let is_new = !map.contains_key(&k);
                if is_new && map.len() >= cfg.max_keys {
                    return Err(Error::Store(StoreErr::MaxKeys));
                }
                map.insert(k.clone(), v);
                log.push(format!("SET {} {}", k, v));
                Ok((Reply::Ok, log.last().cloned()))
            }
            Cmd::Del(k) => {
                let removed = map.remove(&k).is_some();
                log.push(format!("DEL {}", k));
                Ok((
                    if removed { Reply::Int(1) } else { Reply::Int(0) },
                    log.last().cloned(),
                ))
            }
            Cmd::Incr(k) => {
                let v = map.entry(k.clone()).or_insert(0);
                *v += 1;
                Ok((Reply::Int(*v), None))
            }
        }
    }
}

/// 只读代理：同型（`In=Cmd, Out=Result<(Reply,AOF行),Error>`）的替代居留项——
/// GET 返回 Nil，写命令返回 `StoreErr::ReadOnly`（∃ 换装演示，§5.9 型位填充）。
pub struct ReadOnlyProxy;
impl PortCell for ReadOnlyProxy {
    type In = Cmd;
    type Out = Result<(Reply, Option<String>), Error>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), cmd: Cmd) -> Result<(Reply, Option<String>), Error> {
        match cmd {
            Cmd::Get(_) => Ok((Reply::Nil, None)),
            _ => Err(Error::Store(StoreErr::ReadOnly)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// StoreDemux —— Result<(回复,AOF行), Error> → RESP 字符串（无状态多路/编解码）
// ═══════════════════════════════════════════════════════════════════

/// 把引擎输出（`Ok(回复)` 或 `Err(错误)`）编码为 RESP 风格字符串。
pub struct StoreDemux;
impl PortCell for StoreDemux {
    type In = Result<(Reply, Option<String>), Error>;
    type Out = String;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), r: Result<(Reply, Option<String>), Error>) -> String {
        match r {
            Ok((reply, _)) => encode_reply(&reply),
            Err(e) => format!("-ERR {e}\r\n"),
        }
    }
}

/// RESP 风格编码（纯函数，供 StoreDemux 与测试共用）。
pub fn encode_reply(reply: &Reply) -> String {
    match reply {
        Reply::Int(i) => format!(":{i}\r\n"),
        Reply::Ok => "+OK\r\n".to_string(),
        Reply::Nil => "$-1\r\n".to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 单元测试（cargo test --example redis_like）
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_semantics::prelude_all::TryChain;

    #[test]
    fn parse_errors_are_typed_values() {
        assert_eq!(
            CmdParse::step(&mut (), "GET".into()),
            Err(Error::Parse(ParseErr::MissingKey))
        );
        assert_eq!(
            CmdParse::step(&mut (), "SET k notanumber".into()),
            Err(Error::Parse(ParseErr::BadValue("notanumber".into())))
        );
        assert_eq!(
            CmdParse::step(&mut (), "NOPE x".into()),
            Err(Error::Parse(ParseErr::Unknown("NOPE".into())))
        );
        assert_eq!(
            CmdParse::step(&mut (), "   ".into()),
            Err(Error::Parse(ParseErr::EmptyLine))
        );
    }

    #[test]
    fn store_resource_bounds_are_typed_errors() {
        let mut st = new_store(Config { max_keys: 1, max_value: 1_000 });
        assert_eq!(DataStore::step(&mut st, Cmd::Set("a".into(), 1)), Ok((Reply::Ok, Some("SET a 1".into()))));
        // 键数已达上限 → MaxKeys。
        assert_eq!(
            DataStore::step(&mut st, Cmd::Set("b".into(), 2)),
            Err(Error::Store(StoreErr::MaxKeys))
        );
        // 值超限 → ValueTooLarge。
        let mut st2 = new_store(Config { max_keys: 10, max_value: 1_000 });
        assert_eq!(
            DataStore::step(&mut st2, Cmd::Set("c".into(), 999_999)),
            Err(Error::Store(StoreErr::ValueTooLarge(999_999)))
        );
        // INCR 越界不触发资源错误（写原键）。
        assert_eq!(DataStore::step(&mut st2, Cmd::Incr("c".into())), Ok((Reply::Int(1), None)));
    }

    #[test]
    fn try_chain_short_circuits_without_touching_store() {
        // 解析失败 → 短路：DataStore 不执行（存储不变）。
        let mut pipe_state: ((), StoreState) = ((), new_store(Config::default()));
        let out = <TryChain<CmdParse, DataStore> as PortCell>::step(&mut pipe_state, "GET".into());
        assert_eq!(out, Err(Error::Parse(ParseErr::MissingKey)));
        assert!(pipe_state.1.0.is_empty(), "store 必须未被触碰");
        // 成功路径：SET 经两层执行，存储更新。
        let out = <TryChain<CmdParse, DataStore> as PortCell>::step(
            &mut pipe_state,
            "SET k 7".into(),
        );
        assert_eq!(out, Ok((Reply::Ok, Some("SET k 7".into()))));
        assert_eq!(pipe_state.1.0.get("k"), Some(&7));
    }

    #[test]
    fn line_split_buffers_across_chunks() {
        let mut buf = String::new();
        assert_eq!(LineSplit::step(&mut buf, "SET a 1\nGET".into()), vec!["SET a 1"]);
        assert_eq!(LineSplit::step(&mut buf, " b\n".into()), vec!["GET b"]);
        // 冲刷残余：无换行时返回空。
        assert!(LineSplit::step(&mut buf, "".into()).is_empty());
    }

    #[test]
    fn demux_and_codec() {
        assert_eq!(StoreDemux::step(&mut (), Ok((Reply::Int(3), None))), ":3\r\n");
        assert_eq!(StoreDemux::step(&mut (), Err(Error::Parse(ParseErr::MissingKey))), "-ERR GET requires a key\r\n");
    }

    #[test]
    fn read_only_proxy_rejects_writes() {
        assert_eq!(
            ReadOnlyProxy::step(&mut (), Cmd::Set("k".into(), 1)),
            Err(Error::Store(StoreErr::ReadOnly))
        );
        assert_eq!(ReadOnlyProxy::step(&mut (), Cmd::Get("k".into())), Ok((Reply::Nil, None)));
    }
}