//! # Network receive path — system blueprint
//!
//! The receive data path of a kernel network stack (NIC → Ethernet → IP → TCP → application
//! delivery), using pcap file replay as a deterministic data source (similar to kernel
//! pktgen / network testing frameworks).
//!
//! ## The blueprint (abstraction layer)
//!
//! ```text
//!                ┌────────────── receive data path (static topology) ──────────────┐
//!                │                                                      │
//!  pcap file ────► PcapReader ──► EthParser ──► IpParser ──► TcpParser ──► AppDeliver ──► report (stream stats)
//!  (replay src)  │ (phys read)  │ (Eth frame)  │ (IP hdr)   │ (TCP payload) │ (stream agg)   │
//!                │   │            │             │            │            │              │
//!                │   └─ next event (injected by main, one packet at a time)  │            │
//!                │                                            │            └─ stats ──► PktStats (observe)
//!                │                                            │           (Observe stream,     (separate thread,
//!                └────────────────────────────────────────────┴────────────  Dropping carrier)  low-rate stats)
//! ```
//!
//! ## Design highlights
//!
//! 1. **Deterministic replay**: pcap is a real capture format — replaying the same file twice
//!    must yield per-packet identical results (the core means of determinism verification,
//!    same approach as kernel network testing).
//! 2. **Module = protocol layer**: Ethernet/IP/TCP each have one module per layer; after
//!    stripping the header, only the payload is passed downstream — the layering abstraction
//!    of the kernel protocol stack is directly visible in the blueprint.
//! 3. **Observation as data**: `AppDeliver.stats` is an Observe stream; `PktStats` aggregates
//!    at low rate (Dropping carrier — observation does not affect the main path, same pattern
//!    as redis_like).
//! 4. **Parse-only, no validation**: this simplified version does no TCP reordering/checksum
//!    (real-kernel reassembly is a later increment); the current path covers "per-packet parse +
//!    stream aggregation".
//!
//! ## Boundary statements (honest)
//!
//! - Only IPv4 + TCP handled (UDP/other ethertypes are dropped and counted).
//! - No TCP sequence-number reordering (in-order arrival assumed); no checksum verification.
//! - Physical processes (NIC interrupts, DMA, NAPI polling) are not in this blueprint — pcap
//!   replay simulates the "packet entering the protocol stack" boundary condition.

use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::MachinePhysicalSpec;

/// Structural blueprint of the network receive path (DynamicTopology).
pub fn blueprint() -> DynamicTopology {
    DynamicTopology::new()
        // ── machines (6) ──────────────────────────────────────────────
        .with_machine(MachineInstance::new(
            "pcap_reader",
            "pcap_reader",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "eth_parser",
            "eth_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "ip_parser",
            "ip_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "tcp_parser",
            "tcp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "app_deliver",
            "app_deliver",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "pkt_stats",
            "pkt_stats",
            MachinePhysicalSpec::default(),
        ))
        // ── data flow (5 links) ───────────────────────────────────────
        // raw packet → Ethernet → IP → TCP → application
        .with_link(LinkSpec::new(
            ("pcap_reader", "pkt"),
            ("eth_parser", "pkt"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("eth_parser", "ip"),
            ("ip_parser", "ip"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("ip_parser", "tcp"),
            ("tcp_parser", "tcp"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("tcp_parser", "seg"),
            ("app_deliver", "seg"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // observe stream: stream stats → PktStats (low-rate observation, Dropping may discard)
        .with_link(LinkSpec::new(
            ("app_deliver", "stats"),
            ("pkt_stats", "log"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}
