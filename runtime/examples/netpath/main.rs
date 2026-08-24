//! netpath —— 网络接收路径多段管线（阶段 6 硬化：解析失败为类型化错误）。
//!
//! 管线：`EthFrameParse → IpParse → TcpParse`（前三级 `Out = Result<_, NetErr>`，短路）；
//! 经 `TryChain` 组合为单一端口体 `ParseChain`（`Out = Result<TcpSeg, NetErr>`）。
//! **短路载体**（`ResultCarrier` vs `MaybeCarrier`）把这条 Result 车道送入纯消费端
//! `Deliver`（`In = TcpSeg`）——`Ok` 直通、`Err` 短路（单层 `Result<Delivered, NetErr>`，
//! 与 `drive_try` 语义一致）。双载体线路语义等价（T6）；整线重跑验证**确定性**；
//! 解析失败计入类型化错误台账（杜绝零值/空串污染）。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example netpath`

mod cells;

use axiom::cell_core::PortCell;
use axiom_runtime::prelude_all::{MaybeCarrier, ResultCarrier, TryChain, drive_try_carrier};

use cells::{Deliver, EthFrameParse, IpParse, NetErr, Packet, TcpParse, TcpSeg};

/// 解析链：Eth → Ip → Tcp（三级短路，失败为值）。
type ParseChain = TryChain<TryChain<EthFrameParse, IpParse>, TcpParse>;

/// 链状态的具体形态（避免 E0223：别名关联类型需具体化）。
type EthIpState = ((), ()); // EthFrameParse::State, IpParse::State
type ParseChainState = (EthIpState, ()); // (TryChain<Eth,Ip>::State, TcpParse::State)

fn main() {
    let packets = vec![
        Packet { raw: "aa:bb:06:100:8080.10".into() },
        Packet { raw: "aa:bb:06:200:9090.20".into() },
        // 畸形/截断（各阶段都应**类型化拒绝**，而非静默默认）。
        Packet { raw: "malformed-garbage".into() },      // Eth 级缺段
        Packet { raw: "aa:bb:06:badip:8080.10".into() },  // Ip 级源地址非法
        Packet { raw: "aa:bb:06:100:badport.10".into() }, // Tcp 级端口非法
        Packet { raw: "aa:bb:06:100:8080.30".into() },
    ];

    println!("=== netpath: 多段解析管线 (Eth→IP→TCP→Deliver, 失败为值) ===\n");

    // A 线（ResultCarrier）与 B 线（MaybeCarrier）：同一链 × 同一纯消费端。
    let mut chain_a: ParseChainState = (((), ()), ());
    let mut chain_b: ParseChainState = (((), ()), ());
    let mut tbl_a: <Deliver as PortCell>::State = Default::default();
    let mut tbl_b: <Deliver as PortCell>::State = Default::default();
    let mut results_a: Vec<Result<<Deliver as PortCell>::Out, NetErr>> = Vec::new();
    let mut results_b: Vec<Result<<Deliver as PortCell>::Out, NetErr>> = Vec::new();

    for p in &packets {
        let r_a = drive_try_carrier::<ResultCarrier, ParseChain, Deliver, TcpSeg, NetErr>(
            &mut chain_a, &mut tbl_a, p.clone(),
        );
        let r_b = drive_try_carrier::<MaybeCarrier, ParseChain, Deliver, TcpSeg, NetErr>(
            &mut chain_b, &mut tbl_b, p.clone(),
        );
        assert_eq!(r_a, r_b, "两短路载体须语义等价（T6）");
        results_a.push(r_a);
        results_b.push(r_b);
    }

    let delivered: Vec<_> = results_a.iter().filter_map(|r| r.as_ref().ok()).collect();
    for (i, d) in delivered.iter().enumerate() {
        println!(
            "  有效包 {i}: sport={:04} dport={:04} bytes={}",
            d.sport, d.dport, d.bytes
        );
    }
    let errs: Vec<&NetErr> = results_a.iter().filter_map(|r| r.as_ref().err()).collect();
    println!("  错误台账（类型化）: {errs:?}");
    assert_eq!(errs.len(), 3, "三条畸形帧须全部被类型化拒绝");

    // 语义等价：双载体结果一致。
    println!("\n  ResultCarrier 结果: {results_a:?}");
    println!("  MaybeCarrier  结果: {results_b:?}");
    assert_eq!(results_a, results_b, "短路载体语义等价");

    // 确定性：重跑一遍 A 线必须一致。
    let mut chain_a2: ParseChainState = (((), ()), ());
    let mut tbl_a2: <Deliver as PortCell>::State = Default::default();
    let r2: Vec<_> = packets
        .iter()
        .map(|p| {
            drive_try_carrier::<ResultCarrier, ParseChain, Deliver, TcpSeg, NetErr>(
                &mut chain_a2, &mut tbl_a2, p.clone(),
            )
        })
        .collect();
    assert_eq!(results_a, r2, "netpath 必须确定性");
    println!("确定性 ✓（两次一致）+ 载体等价 ✓（Result==Maybe）+ 畸形 3 条类型化拒绝 ✓");
    println!("netpath ok: 多段管线 + 短路载体等价 + 确定性 + 失败为值（阶段 6 硬化）");
}