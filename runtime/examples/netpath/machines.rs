//! # 网络收包路径 — 机器集
//!
//! 蓝图 `blueprint.rs` 中 6 个模块的 Machine 实现：
//!
//! | 模块 | 职责 | 输入 | 输出 | 状态 |
//! |---|---|---|---|---|
//! | `PcapReader` | pcap 文件逐包读取（物理读） | `next` (()) | `pkt` (PktRaw) | 文件句柄 + 游标 |
//! | `EthParser` | 以太帧头剥离（只放行 IPv4） | `pkt` | `ip` (EthOut) | — |
//! | `IpParser` | IP 头剥离（只放行 TCP） | `ip` | `tcp` (IpOut) | — |
//! | `TcpParser` | TCP 载荷提取（按 4 元组定流） | `tcp` | `seg` (TcpSeg) | — |
//! | `AppDeliver` | 流聚合统计 | `seg` | `report` + `stats` | 流 → 字节表 |
//! | `PktStats` | 低速观测（Observe 流，Dropping） | `log` | — | 聚合统计 |

use std::collections::HashMap;
use std::io::Read;

use axiom::declare_ports;
use axiom::machine::{CleanupError, InitError, Machine, MultiOutput, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};

// ════════════════════════════════════════════════════════════════════════
// 数据类型（包 / 协议语义）
// ════════════════════════════════════════════════════════════════════════

/// 原始包：`(pkt_id, 以太帧字节)`。
#[derive(Debug, Clone, PartialEq)]
pub struct PktRaw {
    pub pkt_id: u64,
    pub bytes: Vec<u8>,
}

/// 以太解析结果（已剥 14 字节以太头）。
#[derive(Debug, Clone, PartialEq)]
pub struct EthOut {
    pub pkt_id: u64,
    pub ethertype: u16,
    pub payload: Vec<u8>,
}

/// IP 解析结果（已剥 IP 头）。
#[derive(Debug, Clone, PartialEq)]
pub struct IpOut {
    pub pkt_id: u64,
    pub proto: u8,
    pub src: [u8; 4],
    pub dst: [u8; 4],
    pub payload: Vec<u8>,
}

/// TCP 流标识（4 元组）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowId {
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub sport: u16,
    pub dport: u16,
}

/// TCP 段：流标识 + 应用载荷。
#[derive(Debug, Clone, PartialEq)]
pub struct TcpSeg {
    pub flow: FlowId,
    pub payload: Vec<u8>,
}

/// 应用交付统计（流数 + 总字节）。
#[derive(Debug, Clone, PartialEq)]
pub struct AppReport {
    pub pkt_id: u64,
    pub flows: usize,
    pub bytes: u64,
}

// ════════════════════════════════════════════════════════════════════════
// 模块 1：PcapReader — pcap 文件逐包读取（物理读）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PcapReaderPorts {
        input type PcapReaderInput {
            next [Data] => (), // main 逐包驱动
        }
        output type PcapReaderOutput {
            pkt [Data] => PktRaw,
        }
    }
}

pub struct PcapState {
    pub file: Option<std::fs::File>,
    pub pkt_id: u64,
    pub done: bool,
}

pub struct PcapReader;

impl Machine for PcapReader {
    type State = PcapState;
    type Input = PcapReaderInput;
    type Output = PcapReaderOutput;
    type Ports = PcapReaderPorts;
    type ProcessOutput = SingleOutput<PcapReaderOutput>;

    fn name() -> &'static str { "pcap_reader" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<PcapState, InitError> {
        Ok(PcapState {
            file: None,
            pkt_id: 0,
            done: false,
        })
    }
    #[inline]
    fn process(
        state: &mut PcapState,
        _: &MachineContext,
        input: PcapReaderInput,
    ) -> SingleOutput<PcapReaderOutput> {
        let PcapReaderInput::next(()) = input;
        if state.done {
            return SingleOutput::Idle;
        }
        // 惰性打开 + 跳过 pcap global header（24 字节）
        let f = state.file.get_or_insert_with(|| {
            let mut f = std::fs::File::open("packets.pcap").expect("open packets.pcap");
            let mut g = [0u8; 24];
            f.read_exact(&mut g)
                .expect("read pcap global header");
            // magic 0xa1b2c3d4（little-endian）验证
            assert_eq!(
                u32::from_le_bytes(g[0..4].try_into().unwrap()),
                0xa1b2c3d4,
                "not a pcap file"
            );
            f
        });
        match read_packet(f) {
            Ok(Some(bytes)) => {
                let pkt_id = state.pkt_id;
                state.pkt_id += 1;
                SingleOutput::Yield(PcapReaderOutput::pkt(PktRaw { pkt_id, bytes }))
            }
            Ok(None) => {
                state.done = true;
                SingleOutput::Idle // EOF
            }
            Err(e) => {
                eprintln!("pcap read error: {e}");
                state.done = true;
                SingleOutput::Idle
            }
        }
    }
    fn cleanup(_: PcapState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

/// 读一个 pcap 包记录：`ts_sec(4) ts_usec(4) incl_len(4) orig_len(4) data[incl_len]`。
fn read_packet(f: &mut std::fs::File) -> std::io::Result<Option<Vec<u8>>> {
    let mut hdr = [0u8; 16];
    match f.read_exact(&mut hdr) {
        Ok(_) => {
            let incl = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;
            if incl > 65_535 {
                return Ok(None); // 畸形长度：终止
            }
            let mut data = vec![0u8; incl];
            f.read_exact(&mut data)?;
            Ok(Some(data))
        }
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 2：EthParser — 以太帧头剥离（只放行 IPv4）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct EthParserPorts {
        input type EthParserInput {
            pkt [Data] => PktRaw,
        }
        output type EthParserOutput {
            ip [Data] => EthOut,
        }
    }
}

pub struct EthParser;

impl Machine for EthParser {
    type State = ();
    type Input = EthParserInput;
    type Output = EthParserOutput;
    type Ports = EthParserPorts;
    type ProcessOutput = SingleOutput<EthParserOutput>;

    fn name() -> &'static str { "eth_parser" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: EthParserInput,
    ) -> SingleOutput<EthParserOutput> {
        let EthParserInput::pkt(raw) = input;
        if raw.bytes.len() < 14 {
            return SingleOutput::Idle; // 截断帧：丢弃
        }
        let ethertype = u16::from_be_bytes([raw.bytes[12], raw.bytes[13]]);
        if ethertype != 0x0800 {
            return SingleOutput::Idle; // 非 IPv4：丢弃
        }
        SingleOutput::Yield(EthParserOutput::ip(EthOut {
            pkt_id: raw.pkt_id,
            ethertype,
            payload: raw.bytes[14..].to_vec(),
        }))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 3：IpParser — IP 头剥离（只放行 TCP）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct IpParserPorts {
        input type IpParserInput {
            ip [Data] => EthOut,
        }
        output type IpParserOutput {
            tcp [Data] => IpOut,
        }
    }
}

pub struct IpParser;

impl Machine for IpParser {
    type State = ();
    type Input = IpParserInput;
    type Output = IpParserOutput;
    type Ports = IpParserPorts;
    type ProcessOutput = SingleOutput<IpParserOutput>;

    fn name() -> &'static str { "ip_parser" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: IpParserInput,
    ) -> SingleOutput<IpParserOutput> {
        let IpParserInput::ip(eth) = input;
        let p = &eth.payload;
        if p.len() < 20 {
            return SingleOutput::Idle;
        }
        let ihl = (p[0] & 0x0f) as usize * 4;
        if p.len() < ihl {
            return SingleOutput::Idle;
        }
        let proto = p[9];
        if proto != 6 {
            return SingleOutput::Idle; // 非 TCP：丢弃
        }
        let src = [p[12], p[13], p[14], p[15]];
        let dst = [p[16], p[17], p[18], p[19]];
        SingleOutput::Yield(IpParserOutput::tcp(IpOut {
            pkt_id: eth.pkt_id,
            proto,
            src,
            dst,
            payload: p[ihl..].to_vec(),
        }))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 4：TcpParser — TCP 载荷提取（按 4 元组定流）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct TcpParserPorts {
        input type TcpParserInput {
            tcp [Data] => IpOut,
        }
        output type TcpParserOutput {
            seg [Data] => TcpSeg,
        }
    }
}

pub struct TcpParser;

impl Machine for TcpParser {
    type State = ();
    type Input = TcpParserInput;
    type Output = TcpParserOutput;
    type Ports = TcpParserPorts;
    type ProcessOutput = SingleOutput<TcpParserOutput>;

    fn name() -> &'static str { "tcp_parser" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(
        _: &mut (),
        _: &MachineContext,
        input: TcpParserInput,
    ) -> SingleOutput<TcpParserOutput> {
        let TcpParserInput::tcp(ip) = input;
        let p = &ip.payload;
        if p.len() < 20 {
            return SingleOutput::Idle;
        }
        let sport = u16::from_be_bytes([p[0], p[1]]);
        let dport = u16::from_be_bytes([p[2], p[3]]);
        let doff = (p[12] >> 4) as usize * 4;
        if p.len() < doff {
            return SingleOutput::Idle;
        }
        let flow = FlowId {
            src_ip: ip.src,
            dst_ip: ip.dst,
            sport,
            dport,
        };
        SingleOutput::Yield(TcpParserOutput::seg(TcpSeg {
            flow,
            payload: p[doff..].to_vec(),
        }))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 5：AppDeliver — 流聚合统计（应用交付）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AppDeliverPorts {
        input type AppDeliverInput {
            seg [Data] => TcpSeg,
        }
        output type AppDeliverOutput {
            report [Data]    => AppReport, // 流统计（无下游 → 终端输出，供断言）
            stats  [Observe] => AppReport, // 观测流 → PktStats
        }
    }
}

pub struct AppDeliver;

impl Machine for AppDeliver {
    type State = (HashMap<FlowId, u64>, u64); // (流 → 字节数, 累计包数)
    type Input = AppDeliverInput;
    type Output = AppDeliverOutput;
    type Ports = AppDeliverPorts;
    type ProcessOutput = MultiOutput<AppDeliverOutput>;

    fn name() -> &'static str { "app_deliver" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<Self::State, InitError> {
        Ok((HashMap::new(), 0))
    }
    #[inline]
    fn process(
        state: &mut Self::State,
        _: &MachineContext,
        input: AppDeliverInput,
    ) -> MultiOutput<AppDeliverOutput> {
        let AppDeliverInput::seg(seg) = input;
        let (flows, pkts) = state;
        *flows.entry(seg.flow).or_insert(0) += seg.payload.len() as u64;
        *pkts += 1;
        let total_bytes: u64 = flows.values().sum();
        let report = AppReport {
            pkt_id: *pkts,
            flows: flows.len(),
            bytes: total_bytes,
        };
        MultiOutput::YieldMulti(vec![
            AppDeliverOutput::report(report.clone()),
            AppDeliverOutput::stats(report),
        ])
    }
    fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 模块 6：PktStats — 低速观测（Observe 流，Dropping 载体）
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PktStatsPorts {
        input type PktStatsInput {
            log [Observe] => AppReport,
        }
        output type PktStatsOutput {
            // 纯汇：聚合到 State（观测不反作用）
        }
    }
}

#[derive(Debug, Default)]
pub struct StatsState {
    pub packets: u64,
    pub flows: usize,
    pub bytes: u64,
}

pub struct PktStats;

impl Machine for PktStats {
    type State = StatsState;
    type Input = PktStatsInput;
    type Output = PktStatsOutput;
    type Ports = PktStatsPorts;
    type ProcessOutput = SingleOutput<PktStatsOutput>;

    fn name() -> &'static str { "pkt_stats" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<StatsState, InitError> {
        Ok(StatsState::default())
    }
    #[inline]
    fn process(
        state: &mut StatsState,
        _: &MachineContext,
        input: PktStatsInput,
    ) -> SingleOutput<PktStatsOutput> {
        let PktStatsInput::log(r) = input;
        state.packets += 1;
        state.flows = r.flows;
        state.bytes = r.bytes;
        SingleOutput::Idle
    }
    fn cleanup(_: StatsState, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
