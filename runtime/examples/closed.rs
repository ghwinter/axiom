//! closed —— 封闭边界（`foundations.md` §8）的**整合演示**。
//!
//! 用统一命名的核心/runtime 构造展示 §8 的五个构造概念皆是实例：
//! - **组合自封闭（概念 1/3）**：`Chain`/`Rep`/`Broadcast` 等都是 `PortCell`，可任意嵌套；
//! - **失败为值 + `TryChain` 短路（概念 1）**：`Out=Result`，任一 `Err` 即停、不执行后续；
//! - **同一张图多物理等价（T6）**：同一 `Chain<Double,Triple>` 用 `drive_seq`(内联/同步)
//!   与 `bounded_pump`(跨线程/有界) 各跑一遍 → 输出逐位一致；
//! - **未来内容（概念 4 的 ∃ 侧）**：`SlotDrive<i32,i32>` 运行期安装/换装合规居留项。
//!
//! 全程 zero-cost 静态路径（`Chain` 等单态化）、`#![forbid(unsafe_code)]`、no_std 核心。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example closed`

use axiom::cell_core::{Chain, Conforms, PortCell, Slot, Wire, assert_wiring, drive};
use axiom_runtime::prelude_all::{SlotDrive, SlotPending, TryChain, bounded_pump, drive_seq};

// ═══════════════════════════════════════════════════════════════
// cells（都是 PortCell，概念 1）
// ═══════════════════════════════════════════════════════════════

/// 解析：String -> Result<i32, PErr>（失败为值：概念 1 的全转移 + Out=Result）。
#[derive(Debug, PartialEq)]
struct PErr;
struct Parse;
impl PortCell for Parse {
    type In = &'static str;
    type Out = Result<i32, PErr>;
    type State = ();
    fn step(_: &mut (), s: &'static str) -> Result<i32, PErr> {
        s.parse::<i32>().map_err(|_| PErr)
    }
}

/// 乘 2。
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

/// 乘 3。
struct Triple;
impl PortCell for Triple {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x * 3
    }
}

/// 会失败的乘 2（供 `TryChain`：两个阶段都产生 `Result`）。
struct CheckedDouble;
impl PortCell for CheckedDouble {
    type In = i32;
    type Out = Result<i32, PErr>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> Result<i32, PErr> {
        Ok(x * 2)
    }
}

fn main() {
    println!("=== closed: closed-boundary (§8) integration demo ===\n");

    // ── 1. 组合自封闭（概念 3）＋ 统一 T1 布线判定 ──
    // Chain<Double, Triple> 本身是 PortCell；布线经统一 Conforms<Wire<..>> 编译期判定。
    assert_wiring::<Double, Triple>();
    let _: bool = <() as Conforms<Wire<Double, Triple>>>::OK;
    let mut st = <Chain<Double, Triple> as PortCell>::State::default();
    let one = drive::<Chain<Double, Triple>>(&mut st, 10);
    println!("1. Chain<Double,Triple>(10) = {one}"); // 60

    // ── 2. 失败为值 + TryChain 短路（概念 1）──
    type TP = TryChain<Parse, CheckedDouble>;
    let mut tryst = <TP as PortCell>::State::default();
    let ok = <TP as PortCell>::step(&mut tryst, "42");
    let err = <TP as PortCell>::step(&mut tryst, "xx");
    println!("2. TryChain: \"42\" => {ok:?}  \"xx\" => {err:?}（Err 短路，不执行后续）");

    // ── 3. 同一张图多物理等价（T6）：内联/同步 vs 跨线程/有界 ──
    let inputs = [1i32, 2, 3, 4, 5].to_vec();
    // 3a. 内联/同步：drive_seq 顺序跑 Chain<Double,Triple>
    let mut s_inline = <Chain<Double, Triple> as PortCell>::State::default();
    type Pipe = Chain<Double, Triple>;
    let inline_out: Vec<i32> = drive_seq::<Pipe, i32, i32, _>(&mut s_inline, inputs.clone());
    // 3b. 跨线程/有界：bounded_pump 把 Double 输出经容量 2 的有界队列喂给 Triple
    let outs: Vec<i32> = bounded_pump::<Double, Triple, Vec<i32>, 2>(|| (), || (), inputs.clone());
    assert_eq!(inline_out, outs, "T6：同图不同物理实现须语义等价");
    println!("3. T6：内联/同步 {inline_out:?} == 跨线程/有界 {outs:?}（语义等价 ✓）");

    // ── 4. 未来内容（概念 4 的 ∃ 侧）：运行期安装/换装合规居留项 ──
    let mut slot: SlotDrive<i32, i32> = SlotPending::install::<Double>(()).commit();
    let a = slot.drive(5); // Double: 10
    slot.swap::<Triple>(());
    let b = slot.drive(5); // Triple: 15
    // Slot<i32,i32> 的合规由统一 Conforms 编译期判定（居留项须 In=i32,Out=i32）。
    let _: bool = <Double as Conforms<Slot<i32, i32>>>::OK;
    println!("4. 未来内容：SlotDrive 安装 Double(5)->{a}，换装 Triple(5)->{b}（∃ 选择，Conforms 保证合规）");

    println!("\nclosed ok: 封闭边界整合演示（组合封闭 + 失败短路 + T6 多物理等价 + 未来内容）");
}
