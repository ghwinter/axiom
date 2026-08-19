//! redis_like 用例 —— 基于 cell_core 四构件 + runtime Carrier 重建。
//!
//! 对应旧 redis_like 的结构意图（KV/List/Hash 服务器管线），但完全用新核心表达：
//! 命令管线 = 一组细胞（PortCell），经 Carrier（Inline/Queue）驱动。
//!
//! 模块（细胞）：
//! - `LineSplit`：把原始字节按行拆成命令（有状态：缓冲未结束行）；
//! - `CmdParse`：把一行解析为命令（`GET/SET/DEL/INCR ...`）；
//! - `DataStore`：KV 存储语义（有状态：`HashMap<String, i64>` + 日志输出）；
//! - `RespEncode`：把结果编码为 RESP 风格字符串（无状态、纯变换）。
//!
//! 用于：① 验证 cell_core 能表达"真实多模块管线"；② 驱动 runtime Carrier
//! 迭代（Inline 单线程零分配 vs Queue 跨线程）；③ 保持 redis 语义作构建用例。

use std::collections::HashMap;

use axiom::cell_core::PortCell;

/// 命令：解析后的指令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Get(String),
    Set(String, i64),
    Del(String),
    Incr(String),
    Unknown(String),
}

/// 存储操作结果（RESP 风格）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Int(i64),
    Ok,
    Nil,
    Bytes(String),
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
// CmdParse —— 命令行 → 命令
// ═══════════════════════════════════════════════════════════════════

/// 把一行解析为命令（简化 RESP：`GET key` / `SET key val` / `DEL key` / `INCR key`）。
pub struct CmdParse;
impl PortCell for CmdParse {
    type In = String;
    type Out = Cmd;
    type State = ();
    fn step(_: &mut (), line: String) -> Cmd {
        let mut it = line.split_whitespace();
        match (it.next(), it.next(), it.next()) {
            (Some("GET"), Some(k), _) => Cmd::Get(k.to_string()),
            (Some("SET"), Some(k), Some(v)) => {
                Cmd::Set(k.to_string(), v.parse().unwrap_or(0))
            }
            (Some("DEL"), Some(k), _) => Cmd::Del(k.to_string()),
            (Some("INCR"), Some(k), _) => Cmd::Incr(k.to_string()),
            (Some(other), ..) => Cmd::Unknown(other.to_string()),
            _ => Cmd::Unknown(String::new()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// DataStore —— KV 存储（有状态）+ 日志输出（AOF 精神）
// ═══════════════════════════════════════════════════════════════════

/// KV 存储。State = (map, log)。输出 = (回复, 可选日志行)。
pub struct DataStore;
impl PortCell for DataStore {
    type In = Cmd;
    type Out = (Reply, Option<String>);
    type State = (HashMap<String, i64>, Vec<String>);
    fn step((map, log): &mut (HashMap<String, i64>, Vec<String>), cmd: Cmd) -> (Reply, Option<String>) {
        match cmd {
            Cmd::Get(k) => {
                let r = map.get(&k).copied().map(Reply::Int).unwrap_or(Reply::Nil);
                (r, None)
            }
            Cmd::Set(k, v) => {
                map.insert(k.clone(), v);
                log.push(format!("SET {} {}", k, v));
                (Reply::Ok, log.last().cloned())
            }
            Cmd::Del(k) => {
                let removed = map.remove(&k).is_some();
                log.push(format!("DEL {}", k));
                (
                    if removed { Reply::Int(1) } else { Reply::Int(0) },
                    log.last().cloned(),
                )
            }
            Cmd::Incr(k) => {
                let v = map.entry(k.clone()).or_insert(0);
                *v += 1;
                (Reply::Int(*v), None)
            }
            Cmd::Unknown(s) => (Reply::Bytes(format!("ERR unknown: {s}")), None),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// RespEncode —— 回复 → RESP 字符串（无状态纯变换）
// ═══════════════════════════════════════════════════════════════════

/// 把回复编码为 RESP 风格。
pub struct RespEncode;
impl PortCell for RespEncode {
    type In = (Reply, Option<String>);
    type Out = String;
    type State = ();
    fn step(_: &mut (), (reply, _log): (Reply, Option<String>)) -> String {
        match reply {
            Reply::Int(i) => format!(":{i}\r\n"),
            Reply::Ok => "+OK\r\n".to_string(),
            Reply::Nil => "$-1\r\n".to_string(),
            Reply::Bytes(b) => format!("{b}\r\n"),
        }
    }
}
