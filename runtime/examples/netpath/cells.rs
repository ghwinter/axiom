//! netpath —— 网络接收路径多段管线，用 cell_core + runtime Carrier 重建。
//!
//! 对应旧 netpath（pcap → Ethernet → IP → TCP → deliver + 统计），用新核心表达：
//! 多段**解析管线**（EthFrame → IpParse → TcpParse → Deliver），合成数据包输入。
//! 用 Carrier 多载体驱动同一管线（Inline 静态零分配 vs Queue 队列），
//! 并验证**确定性**（同一输入 → 相同结果）。

use std::collections::HashMap;

use axiom::cell_core::PortCell;

/// 合成数据包（模拟从 pcap 抓包：简化字节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// 模拟：src_mac:dst_mac:proto:sip:dip:sport:dport:payload_len
    pub raw: String,
}

/// 以太网帧解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EthFrame {
    pub proto: u8,
    pub payload: String,
}

/// IP 包解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpPacket {
    pub src: u32,
    pub dst: u32,
    pub proto: u8,
    pub payload: String,
}

/// TCP 段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpSeg {
    pub sport: u16,
    pub dport: u16,
    pub payload: String,
}

/// 递送结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub sport: u16,
    pub dport: u16,
    pub bytes: usize,
}

// ═══════════════════════════════════════════════════════════════
// EthFrameParse —— Packet → EthFrame（无状态）
// ═══════════════════════════════════════════════════════════════

pub struct EthFrameParse;
impl PortCell for EthFrameParse {
    type In = Packet;
    type Out = EthFrame;
    type State = ();
    fn step(_: &mut (), p: Packet) -> EthFrame {
        // 格式：src:dst:proto:rest，取 proto 与后续。
        let parts: Vec<&str> = p.raw.splitn(4, ':').collect();
        let proto = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let payload = parts.get(3).copied().unwrap_or("").to_string();
        EthFrame { proto, payload }
    }
}

// ═══════════════════════════════════════════════════════════════
// IpParse —— EthFrame → IpPacket（无状态）
// ═══════════════════════════════════════════════════════════════

pub struct IpParse;
impl PortCell for IpParse {
    type In = EthFrame;
    type Out = IpPacket;
    type State = ();
    fn step(_: &mut (), f: EthFrame) -> IpPacket {
        // 简化：src从 f.payload 头部数字区取；proto 保留；剩作为 TCP 载荷。
        let (src_str, rest) = f.payload.split_once(':').unwrap_or((&f.payload, ""));
        let src = src_str.parse::<u32>().unwrap_or(0);
        IpPacket { src, dst: 0, proto: f.proto, payload: rest.to_string() }
    }
}

// ═══════════════════════════════════════════════════════════════
// TcpParse —— IpPacket → TcpSeg（无状态）
// ═══════════════════════════════════════════════════════════════

pub struct TcpParse;
impl PortCell for TcpParse {
    type In = IpPacket;
    type Out = TcpSeg;
    type State = ();
    fn step(_: &mut (), ip: IpPacket) -> TcpSeg {
        // payload: "sport.dport"
        let (sp, dp) = ip.payload.split_once('.').unwrap_or((&ip.payload, ""));
        TcpSeg {
            sport: sp.parse().unwrap_or(0),
            dport: dp.parse().unwrap_or(0),
            payload: ip.payload.clone(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Deliver —— TcpSeg → 统计表（有状态：连接 → 字节计数）
// ═══════════════════════════════════════════════════════════════

pub struct Deliver;
impl PortCell for Deliver {
    type In = TcpSeg;
    type Out = Delivered;
    type State = HashMap<(u16, u16), usize>; // (sport,dport) -> bytes
    fn step(tbl: &mut HashMap<(u16, u16), usize>, s: TcpSeg) -> Delivered {
        let key = (s.sport, s.dport);
        let bytes = s.payload.len();
        *tbl.entry(key).or_insert(0) += bytes;
        Delivered { sport: s.sport, dport: s.dport, bytes }
    }
}
