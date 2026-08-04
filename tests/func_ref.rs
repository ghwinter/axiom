//! FuncRef (A4) — borrow-input extension contract tests.

use axiom::func::{Func, FuncRef};

/// A Func that only reads its input: counts bytes (word-count style).
struct ByteCount;

impl Func for ByteCount {
    type Input = String;
    type Output = usize;

    fn name() -> &'static str { "byte_count" }
    fn call(input: String) -> usize {
        input.len()
    }
    fn cost_estimate() -> axiom::func::CostEstimate {
        axiom::func::CostEstimate::Trivial
    }
}

/// The borrow-input path: reads `&String`, produces the same result, and
/// does not mutate the input.
impl FuncRef for ByteCount {
    fn call_ref(input: &String) -> usize {
        input.len()
    }
}

#[test]
fn call_ref_agrees_with_call() {
    let input = String::from("hello world");
    assert_eq!(ByteCount::call(input.clone()), ByteCount::call_ref(&input));
}

#[test]
fn call_ref_does_not_mutate_input() {
    let input = String::from("immutable");
    let _ = ByteCount::call_ref(&input);
    assert_eq!(input, "immutable");
}

#[test]
fn call_ref_borrows_large_input() {
    // A large-ish owned buffer; call_ref must work off the borrow without
    // taking ownership (the caller keeps using it afterwards).
    let mut input = String::with_capacity(1 << 16);
    input.push_str("payload_");
    input.push_str(&"x".repeat(1 << 14));
    let n = ByteCount::call_ref(&input);
    assert_eq!(n, input.len());
    // Caller still owns the buffer.
    input.push('!');
    assert_eq!(input.len(), n + 1);
}

/// A fused two-step chain preferring the borrow path (A4 usage pattern).
struct SplitWs;
impl Func for SplitWs {
    type Input = String;
    type Output = Vec<String>;
    fn name() -> &'static str { "split_ws" }
    fn call(input: String) -> Vec<String> {
        input.split_whitespace().map(str::to_string).collect()
    }
}
impl FuncRef for SplitWs {
    fn call_ref(input: &String) -> Vec<String> {
        input.split_whitespace().map(str::to_string).collect()
    }
}

struct Len;
impl Func for Len {
    type Input = Vec<String>;
    type Output = Vec<usize>;
    fn name() -> &'static str { "len" }
    fn call(input: Vec<String>) -> Vec<usize> {
        input.iter().map(String::len).collect()
    }
}
impl FuncRef for Len {
    fn call_ref(input: &Vec<String>) -> Vec<usize> {
        input.iter().map(String::len).collect()
    }
}

#[test]
fn fused_chain_uses_borrow_path() {
    // The fused chain drives both steps through call_ref: the input String
    // is borrowed end-to-end (no move), the intermediate Vec<String> is
    // borrowed by the second step (no move).
    let line = String::from("alpha beta gamma");
    let lens: Vec<usize> = Len::call_ref(&SplitWs::call_ref(&line));
    assert_eq!(lens, vec![5, 4, 5]);
    // The original line is untouched and still owned by the caller.
    assert_eq!(line, "alpha beta gamma");
}
