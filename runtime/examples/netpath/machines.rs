//! # Network receive path — machine set
//!
//! Machine implementations for the 6 modules in the `blueprint.rs` blueprint:
//!
//! | Module | Responsibility | Input | Output | State |
//! |---|---|---|---|---|
//! | `PcapReader` | Reads pcap file packet by packet (physical read) | `next` (()) | `pkt` (PktRaw) | file handle + cursor |
//! | `EthParser` | Strips Ethernet frame header (passes only IPv4) | `pkt` | `ip` (EthOut) | — |
//! | `IpParser` | Strips IP header (passes only TCP) | `ip` | `tcp` (IpOut) | — |
//! | `TcpParser` | Extracts TCP payload (streams keyed by 4-tuple) | `tcp` | `seg` (TcpSeg) | — |
//! | `AppDeliver` | Stream aggregation statistics | `seg` | `report` + `stats` | stream → bytes table |
//! | `PktStats` | Low-rate observation (Observe stream, Dropping) | `log` | — | aggregate stats |

use std::collections::HashMap;
use std::io::Read;

use axiom::declare_ports;
use axiom::machine::{CleanupError, InitError, Machine, MultiOutput, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};

// ════════════════════════════════════════════════════════════════════════
// Data types (packet / protocol semantics)
// ════════════════════════════════════════════════════════════════════════

/// Raw packet: `(pkt_id, Ethernet frame bytes)`.
#[derive(Debug, Clone, PartialEq)]
pub struct PktRaw {
    pub pkt_id: u64,
    pub bytes: Vec<u8>,
}

/// Ethernet parse result (14-byte Ethernet header already stripped).
#[derive(Debug, Clone, PartialEq)]
pub struct EthOut {
    pub pkt_id: u64,
    pub ethertype: u16,
    pub payload: Vec<u8>,
}

/// IP parse result (IP header already stripped).
#[derive(Debug, Clone, PartialEq)]
pub struct IpOut {
    pub pkt_id: u64,
    pub proto: u8,
    pub src: [u8; 4],
    pub dst: [u8; 4],
    pub payload: Vec<u8>,
}

/// TCP flow identifier (4-tuple).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowId {
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub sport: u16,
    pub dport: u16,
}

/// TCP segment: flow identifier + application payload.
#[derive(Debug, Clone, PartialEq)]
pub struct TcpSeg {
    pub flow: FlowId,
    pub payload: Vec<u8>,
}

/// Application delivery statistics (stream count + total bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct AppReport {
    pub pkt_id: u64,
    pub flows: usize,
    pub bytes: u64,
}

// ════════════════════════════════════════════════════════════════════════
// Module 1: PcapReader — reads pcap file packet by packet (physical read)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PcapReaderPorts {
        input type PcapReaderInput {
            next [Data] => (), // driven by main, one packet at a time
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
        // lazy open + skip the pcap global header (24 bytes)
        let f = state.file.get_or_insert_with(|| {
            let mut f = std::fs::File::open("packets.pcap").expect("open packets.pcap");
            let mut g = [0u8; 24];
            f.read_exact(&mut g)
                .expect("read pcap global header");
            // verify magic 0xa1b2c3d4 (little-endian)
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

/// Read one pcap packet record: `ts_sec(4) ts_usec(4) incl_len(4) orig_len(4) data[incl_len]`.
fn read_packet(f: &mut std::fs::File) -> std::io::Result<Option<Vec<u8>>> {
    let mut hdr = [0u8; 16];
    match f.read_exact(&mut hdr) {
        Ok(_) => {
            let incl = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;
            if incl > 65_535 {
                return Ok(None); // malformed length: terminate
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
// Module 2: EthParser — strips Ethernet frame header (passes only IPv4)
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
            return SingleOutput::Idle; // truncated frame: discard
        }
        let ethertype = u16::from_be_bytes([raw.bytes[12], raw.bytes[13]]);
        if ethertype != 0x0800 {
            return SingleOutput::Idle; // not IPv4: discard
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
// Module 3: IpParser — strips IP header (passes only TCP)
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
            return SingleOutput::Idle; // not TCP: discard
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
// Module 4: TcpParser — extracts TCP payload (streams keyed by 4-tuple)
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
// Module 5: AppDeliver — stream aggregation statistics (application delivery)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AppDeliverPorts {
        input type AppDeliverInput {
            seg [Data] => TcpSeg,
        }
        output type AppDeliverOutput {
            report [Data]    => AppReport, // stream stats (no downstream → terminal output, for assertions)
            stats  [Observe] => AppReport, // observe stream → PktStats
        }
    }
}

pub struct AppDeliver;

impl Machine for AppDeliver {
    type State = (HashMap<FlowId, u64>, u64); // (stream → byte count, cumulative packet count)
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
// Module 6: PktStats — low-rate observation (Observe stream, Dropping carrier)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PktStatsPorts {
        input type PktStatsInput {
            log [Observe] => AppReport,
        }
        output type PktStatsOutput {
            // pure sink: aggregates into State (observation has no side effects)
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
