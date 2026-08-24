//! 有界/背压载体测试（§9.1 载体侧）：`BoundedCarrier` 作为合法 Carrier，
//! 以及 `bounded_pump` 的真实阻塞背压（容量上限限制在飞消息数）。

use axiom::cell_core::PortCell;
use axiom_runtime::prelude_all::{BoundedCarrier, bounded_pump, bounded_pump_try, drive_link};

// 生产 cell：加一（i32 -> i32）。
struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x + 1
    }
}

// 消费 cell：翻倍（i32 -> i32，In == A::Out）。
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
fn bounded_carrier_is_a_valid_carrier() {
    // BoundedCarrier<2> 是合法 Carrier，可被 drive_link 编译期验证驱动。
    let (mut sa, mut sb) = ((), ());
    let out = drive_link::<Inc, Double, BoundedCarrier<2>>(&mut sa, &mut sb, 5);
    assert_eq!(out, 12); // Inc(5->6) -> Bounded<2> -> Double(6->12)
}

#[test]
fn bounded_pump_backpressure_preserves_all_messages() {
    // 容量 CAP=2 的有界泵：生产端满时阻塞（背压），但所有消息最终都被消费、顺序保持。
    let out = bounded_pump::<Inc, Double, Vec<i32>, 2>(|| (), || (), (0..100).collect::<Vec<_>>());
    // 每条 input：Inc(x)=x+1, Double=(x+1)*2。
    assert_eq!(out.len(), 100);
    assert_eq!(out[0], 2);  // 0 -> 1 -> 2
    assert_eq!(out[99], 200); // 99 -> 100 -> 200
}

// 会失败的 cell：把 i32 解析并加一（Out = Result）。
struct FallibleInc;
#[derive(Debug, PartialEq)]
struct Bad;
impl PortCell for FallibleInc {
    type In = i32;
    type Out = Result<i32, Bad>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> Result<i32, Bad> {
        if x < 0 { Err(Bad) } else { Ok(x + 1) }
    }
}

#[test]
fn bounded_pump_try_failure_short_circuits_and_backpressure_holds() {
    // 失败 × 背压联合语义：负输入 Err 短路（不投队列、计数），其余 Ok 在有界队列上背压。
    let (outs, errs) = bounded_pump_try::<FallibleInc, Double, i32, Bad, Vec<i32>, 2>(
        || (), || (), (-1..100).collect::<Vec<_>>()); // 含 1 个负输入（Err 短路）
    // 101 个输入，1 个失败 → 100 个 Ok → 100 个输出。
    assert_eq!(errs, 1);
    assert_eq!(outs.len(), 100);
    // FallibleInc(x)=x+1 (Ok), Double(y)=y*2。input 0 -> Ok(1) -> Double 2。
    assert_eq!(outs[0], 2);
    // 未污染的队列：全部输出皆来自 Ok 的后续 Double。
}

// 消费者会 panic 的 cell：模拟真实 cell 内部的断言失败。
struct PanickyDouble;
impl PortCell for PanickyDouble {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        assert!(x != 2, "consumer trap");
        x * 2
    }
}

#[test]
fn bounded_pump_consumer_panic_resumes_original_payload() {
    // 消费线程 `B::step` panic：泵把**原始载荷**续抛给调用方（`resume_unwind`），
    // 而非吞成通用 "consumer finished"；生产端在断连后停止投递（拆除语义）。
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = bounded_pump::<Inc, PanickyDouble, Vec<i32>, 2>(|| (), || (), vec![0, 1, 2, 3, 4]);
    }));
    let payload = res.expect_err("consumer panic must reach the caller");
    let msg = payload.downcast_ref::<&str>().expect("&str panic payload");
    assert!(msg.contains("consumer trap"), "payload was: {msg}");
}
