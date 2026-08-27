//! KV 协议面（综合用例 SQL-over-Redis 的协议侧）。
//!
//! 迁自 `runtime/examples/redis_like/cells.rs`（原位示例保留）——本模块为组合内计划
//! 副本，双参照标注：原位 = 单用例证据（含三物理驱动与 TCP 接缝）；本处 = 组合计划
//! 的一部分（副本裁剪：StoreDemux / ReadOnlyProxy 未迁入——组合编码由 composite 承接，
//! ∃ 换装非组合焦点）。

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

/// 解析错误（类型化）。
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
}

/// 统一错误：协议面共享的单层类型。
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

/// 服务器配置（资源边界）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 键数量上限（写满即拒绝新增键）。
    pub max_keys: usize,
    /// 值上限（超过即拒绝）。
    pub max_value: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_keys: 10_000,
            max_value: 1_000_000,
        }
    }
}

/// 存储状态：map + AOF 日志 + 配置。
pub type StoreState = (HashMap<String, i64>, Vec<String>, Config);

/// 新建存储状态（AOF 日志从空开始）。
pub fn new_store(cfg: Config) -> StoreState {
    (HashMap::new(), Vec::new(), cfg)
}

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

/// KV 存储（有状态，含资源边界配置）。
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
                    if removed {
                        Reply::Int(1)
                    } else {
                        Reply::Int(0)
                    },
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

/// RESP 风格编码（纯函数，供 composite 与测试共用）。
pub fn encode_reply(reply: &Reply) -> String {
    match reply {
        Reply::Int(i) => format!(":{i}\r\n"),
        Reply::Ok => "+OK\r\n".to_string(),
        Reply::Nil => "$-1\r\n".to_string(),
    }
}