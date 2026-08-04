//! pipelineN 编译期融合——`materialize` 阶段把相邻 `FusedInline` 机器
//! 链替换为单一 `FusedPipeline` 包装器，消除每跳的路由查找与队列开销。
//!
//! # 融合条件
//!
//! 一条 Inline link `(src, src_port) → (dst, dst_port)` 是融合候选当且仅当：
//! 1. `link.kind == Inline`；
//! 2. `src` 和 `dst` 机器均 `is_fused_compatible()`；
//! 3. `src` 在该 `src_port` 上**无其他下游**（无 fan-out）；
//! 4. `dst` 在该端口上**无其他上游**（无 fan-in）。
//!
//! 最大链 = 从链首（无融合候选入边）沿融合候选出边走到链尾（无融合
//! 候选出边）。长度 ≥ 2 的链被替换为 `FusedPipeline`。
//!
//! # 开销消除
//!
//! 非融合每跳：`route_target` 的 2 次 String 克隆 + `VecDeque` push 扩容
//! （均摊 1 次）+ `Box<dyn Any>`（类型擦除固有，1 次）≈ +4 alloc/hop。
//! 融合后链内端口直接 move 投递，消除路由查找与队列开销，仅保留
//! `Box<dyn Any>`（1 次）+ 内部路由（1 次）≈ +2 alloc/hop。
//! 净降 2 alloc/hop（R003 实测验证）。
//!
//! # TupleOutput 处理
//!
//! `FusedInline` 允许 `TupleOutput`（2 个输出）。链中某跳的 2 个输出
//! 可能一个走链内（到下一级），一个是终端（收集为输出）。`FusedPipeline`
//! 用 `internal_link` 记录链内端口，其余端口作为终端输出返回。

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use axiom::port::PortSchema;

use crate::erasure::{ProcessResult, RunningMachine};
use crate::error::RuntimeError;
use crate::topology::PhysicalLink;

/// 融合流水线——把 N 个相邻 `FusedInline` 机器封装为单一 `RunningMachine`。
///
/// `inject` 在单次调用内依次驱动所有级：stage[0] 的输出按 `internal_links`
/// 路由到 stage[1]，依次类推，直到最后一级。链内端口不经过队列/路由
/// 查找；非链内端口（观察端口、终端输出）作为 `ProcessResult` 返回。
pub(crate) struct FusedPipeline {
    /// 链中的各级机器。stage[0] 是入口，stage[n-1] 是出口。
    stages: Vec<Box<dyn RunningMachine>>,
    /// `internal_links[i]` = (stage[i] 喂给 stage[i+1] 的输出端口,
    /// stage[i+1] 接收的输入端口)。长度 = stages.len() - 1。
    internal_links: Vec<(&'static str, String)>,
    /// 流水线名称（用链首机器名，外部链接仍按此名引用）。
    name: String,
    schema: PortSchema,
}

impl FusedPipeline {
    pub(crate) fn new(
        stages: Vec<Box<dyn RunningMachine>>,
        internal_links: Vec<(&'static str, String)>,
        name: String,
    ) -> Self {
        // schema 取第一级的输入 schema（入口端口）——外部链接按此匹配。
        let schema = stages[0].port_schema().clone();
        Self { stages, internal_links, name, schema }
    }
}

impl RunningMachine for FusedPipeline {
    fn name(&self) -> &str { &self.name }

    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult {
        // FusedPipeline 通过 inject 驱动——process_boxed 不直接使用
        // （入口端口由 inject 处理）。保留以防外部直接调用。
        self.stages[0].process_boxed(input)
    }

    fn inject(&mut self, port: &str, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        let mut result = self.stages[0].inject(port, payload);
        let mut terminal: Vec<(&'static str, Box<dyn core::any::Any + Send>)> = Vec::new();

        for i in 0..self.internal_links.len() {
            let (chain_port, next_input) = &self.internal_links[i];
            match result {
                ProcessResult::Idle | ProcessResult::Done => {
                    // 中间级 Idle/Done：链断裂，返回当前结果。
                    return result;
                }
                ProcessResult::Yield { port, value } => {
                    if port == *chain_port {
                        // 链内端口 → 喂给下一级。
                        result = self.stages[i + 1].inject(next_input, value);
                    } else {
                        // 终端端口 → 收集。
                        terminal.push((port, value));
                        result = ProcessResult::Idle;
                    }
                }
                ProcessResult::YieldMulti { outputs } => {
                    // TupleOutput：分拣链内 vs 终端。
                    let mut chain_value: Option<Box<dyn core::any::Any + Send>> = None;
                    for (p, v) in outputs {
                        if p == *chain_port {
                            chain_value = Some(v);
                        } else {
                            terminal.push((p, v));
                        }
                    }
                    match chain_value {
                        Some(v) => {
                            result = self.stages[i + 1].inject(next_input, v);
                        }
                        None => {
                            // 无链内输出——链断裂，返回终端输出。
                            return if terminal.is_empty() {
                                ProcessResult::Idle
                            } else {
                                ProcessResult::YieldMulti { outputs: terminal }
                            };
                        }
                    }
                }
            }
        }

        // 最后一级的输出：非 Idle 的作为终端输出返回。
        match result {
            ProcessResult::Yield { port, value } => {
                terminal.push((port, value));
            }
            ProcessResult::YieldMulti { outputs } => {
                terminal.extend(outputs);
            }
            ProcessResult::Idle | ProcessResult::Done => {}
        }

        if terminal.is_empty() {
            ProcessResult::Idle
        } else if terminal.len() == 1 {
            let (port, value) = terminal.pop().unwrap();
            ProcessResult::Yield { port, value }
        } else {
            ProcessResult::YieldMulti { outputs: terminal }
        }
    }

    fn is_done(&self) -> bool {
        self.stages.iter().any(|s| s.is_done())
    }

    fn is_fused_compatible(&self) -> bool { true }

    fn port_schema(&self) -> &PortSchema { &self.schema }

    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError> {
        let inner = *self;
        for stage in inner.stages {
            stage.cleanup()?;
        }
        Ok(())
    }
}

/// 链识别结果——一组可替换为 `FusedPipeline` 的线性链。
#[derive(Debug)]
pub(crate) struct FusionChain {
    /// 链中机器的名称（按拓扑顺序）。
    machines: Vec<String>,
    /// 链内链接（machine[i] → machine[i+1]）：(src_port, dst_port)。
    internal: Vec<(&'static str, String)>,
}

/// 识别所有可融合的最大线性链。
///
/// 算法：
/// 1. 标记每条 Inline link 是否为融合候选（两端机器可融合 + 无
///    fan-out/fan-in）；
/// 2. 找链首（有融合候选出边但无融合候选入边的机器）；
/// 3. 从链首沿融合候选出边走到链尾，收集链。
///
/// 不修改 `links`——调用方根据返回的链在 `materialize` 中做替换。
pub(crate) fn identify_fusion_chains(
    machine_names: &[String],
    machines: &BTreeMap<String, Box<dyn RunningMachine>>,
    links: &[PhysicalLink],
) -> Vec<FusionChain> {
    // 融合候选链接：Inline + 两端可融合 + 无 fan-out(src_port 唯一) + 无 fan-in(dst 唯一 Inline 入边)
    let is_candidate = |link: &PhysicalLink| -> bool {
        if !matches!(link.kind, axiom::link::LinkKind::Inline) {
            return false;
        }
        let src_ok = machines.get(&link.src_machine)
            .map(|m| m.is_fused_compatible())
            .unwrap_or(false);
        let dst_ok = machines.get(&link.dst_machine)
            .map(|m| m.is_fused_compatible())
            .unwrap_or(false);
        if !src_ok || !dst_ok {
            return false;
        }
        // 无 fan-out：该 src_machine 的 src_port 只有这一条 Inline 出边。
        let fan_out = links.iter().filter(|l| {
            l.src_machine == link.src_machine
                && l.src_port == link.src_port
                && matches!(l.kind, axiom::link::LinkKind::Inline)
        }).count();
        if fan_out > 1 {
            return false;
        }
        // 无 fan-in：该 dst_machine 的 Inline 入边只有这一条。
        let fan_in = links.iter().filter(|l| {
            l.dst_machine == link.dst_machine
                && matches!(l.kind, axiom::link::LinkKind::Inline)
        }).count();
        if fan_in > 1 {
            return false;
        }
        true
    };

    // 候选出边索引：machine_name → (src_port, dst_machine, dst_port)
    let mut candidate_out: BTreeMap<String, (&'static str, String, String)> = BTreeMap::new();
    let mut candidate_in: BTreeMap<String, bool> = BTreeMap::new();
    for link in links {
        if is_candidate(link) {
            // src_port 是 &'static str——从 PortSchema 取，但 PhysicalLink 存的是 String。
            // 这里用 link.src_port 的 &'static str——实际上 MachineWrapper 的 port_name
            // 返回 &'static str。但 PhysicalLink.src_port 是 String。
            // 简化：用 leak 把 String 变成 &'static str（链数量有限，leak 可控）。
            let src_port: &'static str = Box::leak(link.src_port.clone().into_boxed_str());
            candidate_out.insert(
                link.src_machine.clone(),
                (src_port, link.dst_machine.clone(), link.dst_port.clone()),
            );
            candidate_in.insert(link.dst_machine.clone(), true);
        }
    }

    // 找链首：有候选出边但无候选入边。
    let chain_starts: Vec<&String> = machine_names.iter()
        .filter(|name| candidate_out.contains_key(*name) && !candidate_in.get(*name).copied().unwrap_or(false))
        .collect();

    let mut chains = Vec::new();
    for start in chain_starts {
        let mut chain_machines = vec![start.clone()];
        let mut chain_internal = Vec::new();
        let mut current = start.clone();

        loop {
            if let Some(&(src_port, ref dst_machine, ref dst_port)) = candidate_out.get(&current) {
                chain_internal.push((src_port, dst_port.clone()));
                chain_machines.push(dst_machine.clone());
                current = dst_machine.clone();
            } else {
                break;
            }
        }

        if chain_machines.len() >= 2 {
            chains.push(FusionChain {
                machines: chain_machines,
                internal: chain_internal,
            });
        }
    }

    chains
}

/// 对 `LiveTopology` 的 machines 和 links 做融合替换。
///
/// 返回 `(fused_machines, fused_links, fused_topo_order, fused_machine_index, fused_in_degree)`：
/// - 链中机器被移除，替换为单个 `FusedPipeline`（用链首名）；
/// - 链内链接被移除；链外链接指向链首名（链尾的出边）或保持不变（链首的入边）；
/// - `machine_index`/`in_degree`/`topo_order` 相应重建。
pub(crate) fn apply_fusion(
    machines: BTreeMap<String, Box<dyn RunningMachine>>,
    links: Vec<PhysicalLink>,
    topo_order: Vec<String>,
) -> (
    BTreeMap<String, Box<dyn RunningMachine>>,
    Vec<PhysicalLink>,
    Vec<String>,
    BTreeMap<String, usize>,
    Vec<usize>,
) {
    let chains = identify_fusion_chains(&topo_order, &machines, &links);

    if chains.is_empty() {
        // 无可融合链——直接重建索引。
        let machine_index: BTreeMap<String, usize> = topo_order
            .iter().enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let mut in_degree: Vec<usize> = alloc::vec![0; topo_order.len()];
        for link in &links {
            if let Some(&idx) = machine_index.get(&link.dst_machine) {
                in_degree[idx] += 1;
            }
        }
        return (machines, links, topo_order, machine_index, in_degree);
    }

    // 链首 → 链（用于查找哪些机器属于哪条链、哪些链接是链内的）。
    let mut chain_by_head: BTreeMap<String, &FusionChain> = BTreeMap::new();
    // 链中所有机器名（用于移除）。
    let mut fused_machine_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for chain in &chains {
        chain_by_head.insert(chain.machines[0].clone(), chain);
        for name in &chain.machines {
            fused_machine_names.insert(name.clone());
        }
    }

    // 从 machines 中取出链中机器，构建 FusedPipeline。
    let mut machines = machines;
    let mut fused_pipelines: BTreeMap<String, Box<dyn RunningMachine>> = BTreeMap::new();
    for chain in &chains {
        let head = &chain.machines[0];
        let mut stages: Vec<Box<dyn RunningMachine>> = Vec::new();
        for name in &chain.machines {
            let m = machines.remove(name).expect("fused machine exists");
            stages.push(m);
        }
        let pipeline = FusedPipeline::new(
            stages,
            chain.internal.clone(),
            head.clone(),
        );
        fused_pipelines.insert(head.clone(), Box::new(pipeline));
    }

    // 过滤链接：移除链内链接，保留链外链接。
    // 链尾的出边仍用链尾名——需重映射到链首名。
    let mut chain_head_by_member: BTreeMap<String, String> = BTreeMap::new();
    for chain in &chains {
        for name in &chain.machines {
            chain_head_by_member.insert(name.clone(), chain.machines[0].clone());
        }
    }

    // 判断一条 link 是否是链内链接（src 和 dst 在同一条链，且 src 是 dst 的前驱）。
    let is_internal_link = |link: &PhysicalLink| -> bool {
        // 检查是否匹配某条链的 internal link。
        for chain in &chains {
            for (i, (src_port, dst_port)) in chain.internal.iter().enumerate() {
                if link.src_machine == chain.machines[i]
                    && link.src_port.as_str() == *src_port
                    && link.dst_machine == chain.machines[i + 1]
                    && link.dst_port == *dst_port
                {
                    return true;
                }
            }
        }
        false
    };

    let mut fused_links: Vec<PhysicalLink> = Vec::new();
    for link in links {
        if is_internal_link(&link) {
            continue; // 链内链接已内化到 FusedPipeline
        }
        // 重映射 src/dst 到链首名（如果属于某条链）。
        let src_machine = chain_head_by_member.get(&link.src_machine)
            .cloned().unwrap_or(link.src_machine.clone());
        let dst_machine = chain_head_by_member.get(&link.dst_machine)
            .cloned().unwrap_or(link.dst_machine.clone());
        fused_links.push(PhysicalLink {
            src_machine,
            src_port: link.src_port,
            dst_machine,
            dst_port: link.dst_port,
            kind: link.kind,
        });
    }

    // 合并 machines：移除链中机器（已移除），加入 FusedPipeline。
    // machines 中此时只剩非链机器。
    for (head, pipeline) in fused_pipelines {
        machines.insert(head, pipeline);
    }

    // 重建 topo_order：保持原序，但链中机器只保留链首。
    let mut fused_topo_order: Vec<String> = Vec::new();
    for name in &topo_order {
        if fused_machine_names.contains(name) {
            // 只在链首位置保留。
            if chain_by_head.contains_key(name) {
                fused_topo_order.push(name.clone());
            }
        } else {
            fused_topo_order.push(name.clone());
        }
    }

    // 重建索引。
    let machine_index: BTreeMap<String, usize> = fused_topo_order
        .iter().enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut in_degree: Vec<usize> = alloc::vec![0; fused_topo_order.len()];
    for link in &fused_links {
        if let Some(&idx) = machine_index.get(&link.dst_machine) {
            in_degree[idx] += 1;
        }
    }

    (machines, fused_links, fused_topo_order, machine_index, in_degree)
}
