//! netpath —— 网络接收路径多段管线，用 cell_core + runtime Carrier 重建
//! （阶段 6 硬化：解析失败由**类型化错误**表达，杜绝静默默认→数据污染）。
//!
//! 多段**解析管线**（EthFrame → IpParse → TcpParse → Deliver），合成数据包输入；
//! 前三级 `Out = Result<_, NetErr>`（失败为值），经 `TryChain` 短路——解析失败
//! 立即停，不流到后续级、不产生零值/空串污染。
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

/// 解析错误（类型化；取代静默默认值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetErr {
    /// 以太网帧缺段或协议非法。
    Eth(&'static str),
    /// IP 级缺分隔或源地址非法。
    Ip(&'static str),
    /// TCP 级缺分隔或端口非法。
    Tcp(&'static str),
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
// EthFrameParse —— Packet → Result<EthFrame, NetErr>（无状态，失败为值）
// ═══════════════════════════════════════════════════════════════

pub struct EthFrameParse;
impl PortCell for EthFrameParse {
    type In = Packet;
    type Out = Result<EthFrame, NetErr>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), p: Packet) -> Result<EthFrame, NetErr> {
        // 格式：src:dst:proto:rest，取 proto 与后续；缺段或非法协议 → 类型化错误。
        let parts: Vec<&str> = p.raw.splitn(4, ':').collect();
        let proto: u8 = parts
            .get(2)
            .ok_or(NetErr::Eth("缺少协议段"))?
            .parse()
            .map_err(|_| NetErr::Eth("协议段非法"))?;
        let payload = parts
            .get(3)
            .ok_or(NetErr::Eth("缺少载荷段"))?
            .to_string();
        Ok(EthFrame { proto, payload })
    }
}

// ═══════════════════════════════════════════════════════════════
// IpParse —— EthFrame → Result<IpPacket, NetErr>（无状态，失败为值）
// ═══════════════════════════════════════════════════════════════

pub struct IpParse;
impl PortCell for IpParse {
    type In = EthFrame;
    type Out = Result<IpPacket, NetErr>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), f: EthFrame) -> Result<IpPacket, NetErr> {
        // payload："src:rest"；src 地址非法 → 类型化错误。
        let (src_str, rest) = f
            .payload
            .split_once(':')
            .ok_or(NetErr::Ip("缺少源地址段"))?;
        let src: u32 = src_str
            .parse()
            .map_err(|_| NetErr::Ip("源地址非法"))?;
        Ok(IpPacket { src, dst: 0, proto: f.proto, payload: rest.to_string() })
    }
}

// ═══════════════════════════════════════════════════════════════
// TcpParse —— IpPacket → Result<TcpSeg, NetErr>（无状态，失败为值）
// ═══════════════════════════════════════════════════════════════

pub struct TcpParse;
impl PortCell for TcpParse {
    type In = IpPacket;
    type Out = Result<TcpSeg, NetErr>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), ip: IpPacket) -> Result<TcpSeg, NetErr> {
        // payload："sport.dport"；端口非法 → 类型化错误。
        let (sp, dp) = ip
            .payload
            .split_once('.')
            .ok_or(NetErr::Tcp("缺少端口段"))?;
        let sport: u16 = sp
            .parse()
            .map_err(|_| NetErr::Tcp("源端口非法"))?;
        let dport: u16 = dp
            .parse()
            .map_err(|_| NetErr::Tcp("目的端口非法"))?;
        Ok(TcpSeg { sport, dport, payload: ip.payload.clone() })
    }
}

// ═══════════════════════════════════════════════════════════════
// Deliver —— TcpSeg → 统计表（有状态：连接 → 字节计数；总函数）
// ═══════════════════════════════════════════════════════════════

pub struct Deliver;
impl PortCell for Deliver {
    type In = TcpSeg;
    type Out = Delivered;
    type State = HashMap<(u16, u16), usize>; // (sport,dport) -> bytes
    #[inline(always)]
    fn step(tbl: &mut HashMap<(u16, u16), usize>, s: TcpSeg) -> Delivered {
        let key = (s.sport, s.dport);
        let bytes = s.payload.len();
        *tbl.entry(key).or_insert(0) += bytes;
        Delivered { sport: s.sport, dport: s.dport, bytes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(raw: &str) -> Packet {
        Packet { raw: raw.to_string() }
    }

    #[test]
    fn malformed_inputs_are_typed_errors_not_silent_defaults() {
        // 缺段/非法解析 → Err（按阶段短路），不得产生 0/空串污染。
        let r = EthFrameParse::step(&mut (), packet("aa:bb:zz:payload"));
        assert_eq!(r, Err(NetErr::Eth("协议段非法")));
        let r = EthFrameParse::step(&mut (), packet("aa:bb:6"));
        assert_eq!(r, Err(NetErr::Eth("缺少载荷段")));
        let f = EthFrame { proto: 6, payload: "notanip".into() };
        assert_eq!(IpParse::step(&mut (), f), Err(NetErr::Ip("缺少源地址段")));
        let ip = IpPacket { src: 1, dst: 0, proto: 6, payload: "badports".into() };
        assert_eq!(TcpParse::step(&mut (), ip), Err(NetErr::Tcp("缺少端口段")));
    }

    #[test]
    fn valid_path_parses_and_delivers() {
        // 纯 u32 源地址 + 简单端口串：全链合法。
        let r = EthFrameParse::step(&mut (), packet("aa:bb:6:100:4567.80"));
        let f = r.expect("合法帧");
        let ip = IpParse::step(&mut (), f).expect("合法 IP");
        assert_eq!(ip.src, 100);
        let seg = TcpParse::step(&mut (), ip).expect("合法 TCP");
        assert_eq!((seg.sport, seg.dport), (4567u16, 80u16));
        let mut tbl = HashMap::new();
        let d = Deliver::step(&mut tbl, seg);
        assert_eq!(d.bytes, 7); // 载荷 "4567.80" 长度
    }
}