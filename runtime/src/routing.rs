//! 路由与停机传播辅助——Sequential/Parallel 共用的纯函数。
//!
//! 这些函数无状态、无副作用（除 `mark_stopped` 改传入集合），便于
//! 在两种驱动模式下复用，也便于单元测试。

use alloc::collections::BTreeMap;
use alloc::string::String;

use axiom::port::PortDir;

use crate::carrier::ChanSender;
use crate::erasure::{ProcessResult, RunningMachine};
use crate::error::RuntimeError;
use crate::topology::PhysicalLink;

/// Parallel 模式的路由：输出按 (本机器 src_port) 发到下游 carrier；
/// 无下游（终端机器 / 观察端口）则发到结果收集 channel。
/// 消息附带 dst_port 名——下游线程用它 inject。
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

/// 停机传播：标记机器停机，并递归停机"所有入边源均已停机"的下游。
///
/// `pending_sources` 是 `LiveTopology::in_degree` 的克隆副本（按
/// `topo_order` 索引）；`machine_index` 把机器名映射到该索引。
/// 源停机时递减下游的入度，归零表示该机器不再有任何活跃上游 →
/// 它也应停机（级联）。环由 `stopped` 位集（按 ID，P0）终止。
///
/// 用索引数组而非 `BTreeMap<String, usize>` 承载入度——`materialize`
/// 一次性建表，tick 热路径只克隆 `Vec<usize>`（单次分配，与链路数
/// 无关），保证 R002 "每链接常数分配"不变量。
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

/// 检测拓扑是否含环（基于 Kahn 算法——与 core 的 `detect_cycle` 一致）。
///
/// 有环时 Parallel 模式无法靠 channel 断开级联停机（环中线程互相
/// 保活），需改用全局 stop_signal + tick 限制驱动。无环时保持现有
/// 级联停机路径。
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

/// 校验链接端点：机器存在 + 端口存在 + 方向匹配（src 端是输出，dst 端是输入）。
///
/// 之前的实现只取了 machine 却丢弃 port_schema（`let _ =`），端口名从未
/// 被校验——链接引用了不存在的端口时物化不会报错，直到 tick 时 inject
/// 静默返回 Idle（消息被吞）。现在显式校验方向，让 `DanglingRef` 在
/// 物化阶段就暴露无效端口。
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
