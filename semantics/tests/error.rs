//! 错误/短路通路（drive_try）测试：`Out = Result` 约定 + 短路，闭合 §9.2 物理侧。

use axiom::cell_core::PortCell;
use axiom_semantics::prelude_all::{TryChain, drive_try};

// 解析 cell：把 &str 解析成 i32，失败返回 Err（带错误语义）。
struct Parse;
#[derive(Debug, PartialEq)]
struct ParseErr;
impl PortCell for Parse {
    type In = &'static str;
    type Out = Result<i32, ParseErr>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), input: &'static str) -> Result<i32, ParseErr> {
        input.parse::<i32>().map_err(|_| ParseErr)
    }
}

// 后续 cell：把 i32 翻倍（消费 Parse 的 Ok）。
struct Double;
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

#[test]
fn drive_try_ok_threads_and_short_circuits_on_err() {
    let (mut sp, mut sd) = ((), ());
    // 合法："42" -> Ok(42) -> Double 84
    let ok = drive_try::<Parse, Double, i32, ParseErr>(&mut sp, &mut sd, "42");
    assert_eq!(ok, Ok(84));
    // 非法："xx" -> Err 直接短路，不流入 Double。
    let err = drive_try::<Parse, Double, i32, ParseErr>(&mut sp, &mut sd, "xx");
    assert_eq!(err, Err(ParseErr));
}

// 会失败的 cell：把 i32 翻倍（Out = Result）。
struct CheckedDouble;
impl PortCell for CheckedDouble {
    type In = i32;
    type Out = Result<i32, ParseErr>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> Result<i32, ParseErr> {
        Ok(x * 2)
    }
}

#[test]
fn try_chain_single_level_short_circuit() {
    // TryChain<Parse, CheckedDouble> 是一个 PortCell：In=&str, Out=Result<i32>（单层）。
    type Pipe = TryChain<Parse, CheckedDouble>;
    let mut st = <Pipe as PortCell>::State::default();
    // 合法："42" -> Ok(42) -> Ok(84)（单层 Ok，不在嵌套）。
    assert_eq!(<Pipe as PortCell>::step(&mut st, "42"), Ok(84));
    // 非法："xx" -> Err(ParseErr)（单层，短路不流入 CheckedDouble）。
    assert_eq!(<Pipe as PortCell>::step(&mut st, "xx"), Err(ParseErr));
}
