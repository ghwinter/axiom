//! netpath —— 网络接收路径多段管线，用 cell_core + Carrier 重建。
//!
//! 重建旧 netpath（pcap→Eth→IP→TCP→deliver）的本质：多段解析管线 + 确定性。
//! 用 Carrier **多载体驱动同一皮层链**（InlineCarrier 静态零分配 vs QueueCarrier
//! 队列），语义等价（T6），并验证**确定性**（同一输入→相同结果）。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example netpath`

mod cells;

use axiom::cell_core::PortCell;
use axiom_runtime::carrier::{Carrier, InlineCarrier, QueueCarrier};
use axiom_runtime::flow::drive_link;

use cells::{Deliver, EthFrameParse, IpParse, Packet, TcpParse};

fn main() {
    let packets = vec![
        Packet { raw: "aa:bb:06:100:8080.10".into() },
        Packet { raw: "aa:bb:06:200:8080.20".into() },
        Packet { raw: "aa:bb:06:100:9090.30".into() },
    ];

    println!("=== netpath: 多段解析管线 (Eth→IP→TCP→Deliver) ===\n");

    // 用 InlineCarrier 与 QueueCarrier 驱动同一个"EthFrameParse→IpParse"皮层链，
    // 后续 TCP/Deliver 在调用线程完成——两载体语义等价（T6）。
    let (mut eth_s, mut ip_s) = ((), ());
    let (mut qt_s, mut qi_s) = ((), ());
    let (mut tcp_s_i, mut dlv_s_i) = ((), Default::default());
    let (mut tcp_s_q, mut dlv_s_q) = ((), Default::default());

    let mut inline_results = Vec::new();
    let mut queue_results = Vec::new();

    for p in &packets {
        // Inline 路径：EthFrameParse -> IpParse（drive_link 编译期验证）
        let ip = drive_link::<EthFrameParse, IpParse, InlineCarrier>(&mut eth_s, &mut ip_s, p.clone());
        let seg = TcpParse::step(&mut tcp_s_i, ip);
        inline_results.push(Deliver::step(&mut dlv_s_i, seg));

        // Queue 路径：同一皮层链，经队列中转（每消息 Box 分配）
        let seg_q = {
            let qip =
                <QueueCarrier as Carrier<EthFrameParse, IpParse>>::flow(&mut qt_s, &mut qi_s, p.clone());
            TcpParse::step(&mut tcp_s_q, qip)
        };
        queue_results.push(Deliver::step(&mut dlv_s_q, seg_q));
    }

    for (i, d) in inline_results.iter().enumerate() {
        println!("  包 {i}: sport={:04} dport={:04} bytes={}", d.sport, d.dport, d.bytes);
    }

    // 语义等价：两个载体结果一致。
    println!("\n  Inline 结果: {inline_results:?}");
    println!("  Queue  结果: {queue_results:?}");
    assert_eq!(inline_results, queue_results, "不同载体须语义等价");

    // 确定性：重跑一遍 Inline 必须一致。
    let mut eth_s2 = ();
    let mut ip_s2 = ();
    let mut tcp_s2 = ();
    let mut dlv_s2 = Default::default();
    let r2: Vec<_> = packets.iter().map(|p| {
        let ip = drive_link::<EthFrameParse, IpParse, InlineCarrier>(&mut eth_s2, &mut ip_s2, p.clone());
        let seg = TcpParse::step(&mut tcp_s2, ip);
        Deliver::step(&mut dlv_s2, seg)
    }).collect();
    assert_eq!(inline_results, r2, "netpath 必须确定性");
    println!("确定性 ✓（两次一致） + 载体等价 ✓（Inline==Queue）");
    println!("netpath ok: 多段解析管线 + 多载体等价 + 确定性 基于 cell_core + Carrier");
}
