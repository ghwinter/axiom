//! 组合核心：SQL-over-Redis 单一组合 `PortCell`。
//!
//! 协议面（行级命令）经 [`RouteParse`] 分派：SQL 关键字开头的行 → [`sql_plan`] 计算面；
//! 其余 → [`redis_plan`] KV 面。两面状态共居 [`CompositeState`]，由 [`ComposeLine`]
//! （单一 `PortCell`：In = String，Out = RESP 风格字符串）承载——"计算管线 × 网络服务"
//! 同一组合核心，多物理驱动（sync 三驱动 / async 馈入）行级等价（T6）。

use axiom::cell_core::PortCell;

use crate::redis_plan::{self, Cmd as KvCmd, Error, Reply};
use crate::sql_plan::{self, ExecOut, PErr};

/// 组合状态：SQL 管线状态 + KV 存储状态（两计算面共居）。
pub type CompositeState = (sql_plan::SqlPipeState, redis_plan::StoreState);

/// 新建组合状态（空库 + 空存储）。
pub fn new_composite_state() -> CompositeState {
    (
        sql_plan::new_pipe_state(),
        redis_plan::new_store(redis_plan::Config::default()),
    )
}

/// 组合命令路由：协议面行 → KV 命令 或 SQL 文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// KV 命令（GET/SET/DEL/INCR）。
    Kv(KvCmd),
    /// SQL 文本（整行交给计算面管线）。
    Sql(String),
}

/// 分派 cell：行首关键字 ∈ SQL 语义则整行视为 SQL 文本，否则走 KV 解析（失败为值）。
pub struct RouteParse;
impl PortCell for RouteParse {
    type In = String;
    type Out = Result<Route, Error>;
    type State = ();
    fn step(_: &mut (), line: String) -> Result<Route, Error> {
        let head = line
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase();
        match head.as_str() {
            "select" | "insert" | "create" | "update" | "delete" | "drop" | "alter" => {
                Ok(Route::Sql(line))
            }
            _ => redis_plan::CmdParse::step(&mut (), line).map(Route::Kv),
        }
    }
}

/// 编码 KV 存储结果（Ok → RESP 风格；Err → `-ERR <e>\r\n`）。
pub fn encode_kv(r: Result<(Reply, Option<String>), Error>) -> String {
    match r {
        Ok((reply, _)) => redis_plan::encode_reply(&reply),
        Err(e) => format!("-ERR {e}\r\n"),
    }
}

/// 编码 SQL 执行结果（Ok → `+<msg>\r\n` / `*n rows: …\r\n`；Err → `-ERR <e:?>\r\n`）。
pub fn encode_sql(r: Result<ExecOut, PErr>) -> String {
    match r {
        Ok(ExecOut::Ok(m)) => format!("+{m}\r\n"),
        Ok(ExecOut::Rows(rows)) => format!("*{} rows: {:?}\r\n", rows.len(), rows),
        Err(e) => format!("-ERR {e:?}\r\n"),
    }
}

/// 组合执行 cell（分派后）：`In = Route` → RESP 风格应答。
pub struct ExecCell;
impl PortCell for ExecCell {
    type In = Route;
    type Out = String;
    type State = CompositeState;
    fn step((sql_pipe, store): &mut CompositeState, route: Route) -> String {
        match route {
            Route::Kv(cmd) => encode_kv(redis_plan::DataStore::step(store, cmd)),
            Route::Sql(sql) => encode_sql(sql_plan::SqlPipe::step(sql_pipe, sql)),
        }
    }
}

/// 组合解码 cell（供链路装配）：`In = Result<Route, Error>`（`RouteParse` 的输出）→ 应答。
pub struct DemuxCell;
impl PortCell for DemuxCell {
    type In = Result<Route, Error>;
    type Out = String;
    type State = CompositeState;
    fn step(st: &mut CompositeState, r: Result<Route, Error>) -> String {
        match r {
            Err(e) => format!("-ERR {e}\r\n"),
            Ok(route) => ExecCell::step(st, route),
        }
    }
}

/// 组合核心（单一 `PortCell`）：行 → RESP 风格应答（两计算面经同一状态）。
pub struct ComposeLine;
impl PortCell for ComposeLine {
    type In = String;
    type Out = String;
    type State = CompositeState;
    fn step(st: &mut CompositeState, line: String) -> String {
        DemuxCell::step(st, RouteParse::step(&mut (), line))
    }
}

/// 确定性语料（KV + SQL + 各层错误样本，每 i 拍混合）。
pub fn build_corpus(n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..n {
        out.push(format!("SET k{} {}", i % 7, (i as i64) * 3 % 999));
        out.push(format!("GET k{}", i % 7));
        if i % 5 == 0 {
            out.push(format!("INCR k{}", i % 7));
        }
        if i % 9 == 0 {
            out.push("CREATE TABLE users (id, val)".to_string());
            out.push(format!("INSERT INTO users VALUES ({})", i));
        }
        if i % 4 == 0 {
            out.push("SELECT * FROM users".to_string());
        }
        if i % 6 == 0 {
            out.push("GET".to_string()); // KV 解析短路
        }
        if i % 7 == 0 {
            out.push("SET k bad".to_string()); // KV 解析短路
        }
        if i % 11 == 0 {
            out.push("SELECT * FROM 'oops".to_string()); // 词法短路（计算面）
        }
        if i % 13 == 0 {
            out.push("SELECT * FROM missing".to_string()); // 执行错误（表不存在）
        }
        if i % 17 == 0 {
            out.push("INSERT INTO users VALUES".to_string()); // 语法短路（计算面）
        }
    }
    out
}