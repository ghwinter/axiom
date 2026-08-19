//! Routing and shutdown propagation helpers — pure functions shared by Sequential/Parallel.
//!
//! These functions are stateless and side-effect-free (except `mark_stopped`, which mutates
//! the passed-in set), so they can be reused in both driving modes and are easy to unit test.

use alloc::collections::BTreeMap;
use alloc::string::String;

use axiom::port::PortDir;

use crate::carrier::ChanSender;
use crate::erasure::{ProcessResult, RunningMachine};
use crate::error::RuntimeError;
use crate::topology::PhysicalLink;

/// Routing in Parallel mode: outputs are sent to downstream carriers by (this machine's src_port);
/// with no downstream (terminal machine / observation port) they go to the result collection channel.
/// Messages carry the dst_port name — the downstream thread uses it to inject.
pub(crate) fn route_parallel_outputs(
    result: ProcessResult,
    my_routes: &BTreeMap<String, (String, ChanSender)>,
    result_tx: &std::sync::mpsc::Sender<ProcessResult>,
) {
    match result {
        ProcessResult::Idle | ProcessResult::Done => {}
        ProcessResult::Yield { port, value } => {
            match my_routes.get(port) {
                Some((dst_port, tx)) => {
                    tx.send((dst_port.clone(), value));
                }
                None => {
                    let _ = result_tx.send(ProcessResult::Yield { port, value });
                }
            }
        }
        ProcessResult::YieldMulti { outputs: list } => {
            for (port, value) in list {
                match my_routes.get(port) {
                    Some((dst_port, tx)) => {
                        tx.send((dst_port.clone(), value));
                    }
                    None => {
                        let _ = result_tx.send(ProcessResult::Yield { port, value });
                    }
                }
            }
        }
    }
}

/// Shutdown propagation: mark a machine stopped, then recursively stop any downstream
/// whose sources are all stopped.
///
/// `pending_sources` is a cloned copy of `LiveTopology::in_degree` (indexed by
/// `topo_order`); `machine_index` maps machine names to that index.
/// When a source stops, it decrements the downstream's in-degree; reaching zero means
/// the machine no longer has any active upstream → it should stop too (cascade). Cycles
/// are terminated by the `stopped` bit set (by ID, P0).
///
/// The in-degree is carried by an index array rather than `BTreeMap<String, usize>` —
/// `materialize` builds the table once, and the tick hot path only clones `Vec<usize>`
/// (a single allocation, independent of link count), preserving the "constant allocation
/// per link" invariant of R002.
pub(crate) fn mark_stopped(
    machine_id: usize,
    machine_name: &str,
    stopped: &mut [bool],
    pending_sources: &mut [usize],
    machine_index: &BTreeMap<String, usize>,
    links: &[PhysicalLink],
) {
    if !stopped[machine_id] {
        stopped[machine_id] = true;
        for link in links.iter().filter(|l| l.src_machine == machine_name) {
            if let Some(&idx) = machine_index.get(&link.dst_machine) {
                let deg = &mut pending_sources[idx];
                *deg -= 1;
                if *deg == 0 {
                    mark_stopped(idx, &link.dst_machine, stopped, pending_sources, machine_index, links);
                }
            }
        }
    }
}

/// Detect whether the topology contains a cycle (based on Kahn's algorithm — consistent with
/// core's `detect_cycle`).
///
/// With a cycle, Parallel mode cannot rely on channels to break cascaded shutdown (threads in
/// a cycle keep each other alive), so it must fall back to a global stop_signal + tick limit.
/// Without a cycle, the existing cascaded shutdown path is kept.
pub(crate) fn has_cycle(
    machine_names: &[String],
    links: &[PhysicalLink],
) -> bool {
    let mut in_degree: BTreeMap<String, usize> = machine_names
        .iter().map(|n| (n.clone(), 0)).collect();
    for link in links {
        *in_degree.get_mut(&link.dst_machine).unwrap_or(&mut 0) += 1;
    }
    let mut queue: std::collections::VecDeque<String> = in_degree
        .iter().filter(|&(_, &d)| d == 0).map(|(n, _)| n.clone()).collect();
    let mut visited = 0usize;
    while let Some(name) = queue.pop_front() {
        visited += 1;
        for link in links.iter().filter(|l| l.src_machine == name) {
            if let Some(d) = in_degree.get_mut(&link.dst_machine) {
                *d -= 1;
                if *d == 0 {
                    queue.push_back(link.dst_machine.clone());
                }
            }
        }
    }
    visited < machine_names.len()
}

/// Validate link endpoints: machine exists + port exists + direction matches (src side is an
/// output, dst side is an input).
///
/// The previous implementation only took the machine and discarded the port_schema (`let _ =`),
/// so port names were never validated — links referencing a nonexistent port did not error
/// during materialization, and inject would silently return Idle at tick time (message swallowed).
/// Now the direction is explicitly validated so `DanglingRef` exposes invalid ports at
/// materialization time.
pub(crate) fn validate_endpoint(
    machines: &BTreeMap<String, Box<dyn RunningMachine>>,
    machine: &str,
    port: &str,
    expected_dir: PortDir,
) -> Result<(), RuntimeError> {
    let m = machines.get(machine)
        .ok_or_else(|| RuntimeError::DanglingRef {
            machine: machine.to_string(),
            port: port.to_string(),
        })?;
    let decl = m.port_schema().find(port).ok_or_else(|| RuntimeError::DanglingRef {
        machine: machine.to_string(),
        port: port.to_string(),
    })?;
    if decl.dir != expected_dir {
        return Err(RuntimeError::DanglingRef {
            machine: machine.to_string(),
            port: port.to_string(),
        });
    }
    Ok(())
}
