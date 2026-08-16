//! axiom 统一 runtime 主体——`Runtime` 结构与 `materialize`/`tick`/`shutdown` 生命周期。
//!
//! 驱动循环按 `RuntimeConfig::mode` 分发：
//! - `Sequential` / `Inline` → 单线程 BFS（直接 move 投递）；
//! - `Parallel(n)` → 每机器一个 OS 线程 + channel 载体。

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use axiom::port::PortDir;

use crate::carrier::{channel_for, ChanReceiver, ChanSender, RoutedMsg};
use crate::config::{ExecMode, RuntimeConfig};
use crate::erasure::{ProcessResult, RunningMachine};
use crate::error::RuntimeError;
use crate::io::{IoInterest, IoReactor, IoToken, RawIo};
use crate::registry::Registry;
use crate::routing::{has_cycle, mark_stopped, route_parallel_outputs, validate_endpoint};
use crate::topology::{LiveTopology, PhysicalLink, TopologyIds};

/// axiom 统一 runtime。
pub struct Runtime {
    config: RuntimeConfig,
    registry: Registry,
    topology: Option<LiveTopology>,
    /// IO 多路复用路由表：token → (machine_name, port_name)。
    /// `register_io` 填充，`run_io` 查询——把 reactor 就绪事件转为
    /// `(machine, port, IoEvent)` 输入注入 tick 循环。
    io_routing: BTreeMap<IoToken, (String, String)>,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config, registry: Registry::new(), topology: None, io_routing: BTreeMap::new() }
    }

    pub fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }

    pub fn config(&self) -> &RuntimeConfig { &self.config }

    pub fn register<M>(&mut self, machine_type: &str)
    where
        M: axiom::machine::Machine,
        M::Input: core::any::Any + Send,
        M::Output: core::any::Any + Send,
    {
        self.registry.register::<M>(machine_type);
    }

    /// 注册一个可融合机器——`M: FusedInline` 在类型层保证输出数量固定，
    /// `materialize` 可将其纳入 `FusedPipeline` 链（消除每跳路由开销）。
    pub fn register_fused<M>(&mut self, machine_type: &str)
    where
        M: axiom::machine::Machine + axiom::machine::FusedInline,
        M::Input: core::any::Any + Send + axiom::portset::Pack,
        M::Output: core::any::Any + Send + axiom::portset::Unpack,
        M::ProcessOutput: axiom::machine::FusedCompatible,
    {
        self.registry.register_fused::<M>(machine_type);
    }

    /// 注册一个复合 Machine——子拓扑 + 端口映射封装为单一 `machine_type`。
    ///
    /// `materialize` 遇到该类型的实例时递归展开：子机器名字空间化为
    /// `parent.sub`，外部链接按端口映射表重定向到子机器。展开在机器
    /// 构建、端点校验、融合之前——融合看到的是展开后的扁平拓扑，
    /// `FusedPipeline` 可跨原复合边界融合。
    ///
    /// 嵌套复合（子拓扑中再次使用复合 `machine_type`）通过循环展开
    /// 处理，直到无复合残留（深度上限 64，防止配置错误导致无限展开）。
    pub fn register_composite(&mut self, machine_type: &str, spec: axiom::composite::CompositeSpec) {
        self.registry.register_composite(machine_type, spec);
    }

    /// 物化 DeploySpec——把纯数据拓扑解释为运行时实体。
    ///
    /// 物化流程：
    /// 1. `DeploySpec::validate()` 结构检查（名称唯一性、自环、度约束）；
    /// 2. **复合展开**——把 `register_composite` 注册的复合实例替换为
    ///    名字空间化的子拓扑 + 重定向外部链接（递归到任意深度）；
    /// 3. 按展开后的 machine_type 构建机器实例；
    /// 4. `validate_endpoint` 校验端口存在性 + 方向；
    /// 5. `apply_fusion` 融合相邻 FusedInline 链。
    pub fn materialize(&mut self, spec: &axiom::deploy::DeploySpec) -> Result<(), RuntimeError> {
        spec.validate().map_err(|e| RuntimeError::InitFailed {
            machine: "<spec>".into(),
            error: axiom::machine::InitError::Other(format!("validate failed: {e:?}")),
        })?;

        // 复合展开：把 composite machine_type 实例替换为子拓扑（名字空间化）。
        // 展开在 validate 之后（原始结构合法）、机器构建之前（展开后才是
        // 真实拓扑）。融合看到的是展开后的扁平拓扑——复合边界已消失，
        // FusedPipeline 可跨原复合边界融合。
        let (expanded_machines, expanded_links) = axiom::composite::expand_composites(
            spec.machines.clone(),
            spec.links.clone(),
            self.registry.composites(),
        ).map_err(|e| match e {
            axiom::composite::CompositeError::TooDeep { depth, hint } => {
                RuntimeError::CompositeTooDeep { depth, hint }
            }
            other => RuntimeError::InitFailed {
                machine: "<composite>".into(),
                error: axiom::machine::InitError::Other(format!("composite error: {other}")),
            },
        })?;

        let mut machines: BTreeMap<String, Box<dyn RunningMachine>> = BTreeMap::new();

        for instance in &expanded_machines {
            let machine_type = instance.machine_type.as_ref();
            let name = instance.name.as_ref();

            // MachineContext 接受 Cow：直接克隆 instance.name（Cow），
            // 借用时零复制、owned 时一次转移——无需 leak 到 'static。
            let ctx = axiom::port::MachineContext::new(instance.name.clone());

            let machine = self.registry.build(machine_type, ctx)?;
            machines.insert(name.to_string(), machine);
        }

        for link in &expanded_links {
            validate_endpoint(&machines, link.out.0.as_ref(), link.out.1.as_ref(), PortDir::Out)?;
            validate_endpoint(&machines, link.into.0.as_ref(), link.into.1.as_ref(), PortDir::In)?;
        }

        let topo_order: Vec<String> = expanded_machines.iter()
            .map(|m| m.name.as_ref().to_string())
            .collect();

        let links: Vec<PhysicalLink> = expanded_links.iter()
            .map(|l| PhysicalLink {
                src_machine: l.out.0.as_ref().to_string(),
                src_port: l.out.1.as_ref().to_string(),
                dst_machine: l.into.0.as_ref().to_string(),
                dst_port: l.into.1.as_ref().to_string(),
                kind: l.kind.clone(),
            })
            .collect();

        // pipelineN 融合：把相邻 FusedInline 机器链替换为 FusedPipeline，
        // 消除每跳的路由查找与队列开销。apply_fusion 重建 machines/links/
        // topo_order/machine_index/in_degree，保证后续 tick 看到的是融合后
        // 的拓扑（链外链接指向链首名，链内链接已内化）。
        let (machines, links, topo_order, machine_index, in_degree) =
            crate::fusion::apply_fusion(machines, links, topo_order);

        // P2：构建路由索引——物化期事实（含融合后拓扑），tick 热路径
        // O(log L) 查找，消除 route_target 的 O(L) 线性扫描。
        let mut route_map: BTreeMap<String, BTreeMap<String, (String, String)>> = BTreeMap::new();
        for l in &links {
            route_map
                .entry(l.src_machine.clone())
                .or_default()
                .insert(l.src_port.clone(), (l.dst_machine.clone(), l.dst_port.clone()));
        }

        // P0：ID 化路由索引——tick 热路径免字符串匹配与 String clone。
        // 机器按 topo_order 序编号（= machine_index 值），端口按 schema 的
        // inputs()/outputs() 序编号（&'static str，编译期已知）。
        let mut route_by_src: Vec<Vec<(&'static str, usize, u16)>> =
            vec![Vec::new(); machines.len()];
        let mut out_port_names: Vec<Vec<&'static str>> = Vec::with_capacity(machines.len());
        let mut in_port_names: Vec<Vec<&'static str>> = Vec::with_capacity(machines.len());
        for name in &topo_order {
            let schema = machines[name].port_schema();
            out_port_names.push(schema.outputs().map(|p| p.name).collect());
            in_port_names.push(schema.inputs().map(|p| p.name).collect());
        }
        for l in &links {
            let src_id = *machine_index.get(&l.src_machine).expect("link src machine indexed");
            let dst_id = *machine_index.get(&l.dst_machine).expect("link dst machine indexed");
            // src_port（String）匹配 schema 序的 &'static str（validate_endpoint
            // 已保证存在）；dst_port 解析为目标机器输入端口 ID。
            let src_port = out_port_names[src_id]
                .iter()
                .copied()
                .find(|p| *p == l.src_port.as_str())
                .unwrap_or("");
            let dst_pid = in_port_names[dst_id]
                .iter()
                .position(|p| *p == l.dst_port.as_str())
                .unwrap_or(0) as u16;
            route_by_src[src_id].push((src_port, dst_id, dst_pid));
        }
        let ids = TopologyIds { route_by_src, out_port_names, in_port_names };

        self.topology = Some(LiveTopology {
            machines,
            links,
            topo_order,
            machine_index,
            in_degree,
            route_map,
            ids,
        });
        Ok(())
    }

    pub fn topology(&self) -> Option<&LiveTopology> { self.topology.as_ref() }

    /// 驱动循环：按 `RuntimeConfig::mode` 分发。
    ///
    /// - `Sequential` / `Inline`：单线程 BFS 驱动（直接 move 投递）。
    /// - `Parallel(n)`：每机器一个线程 + channel 载体（见 [`Self::drive_parallel`]）。
    pub fn tick(
        &mut self,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        match self.config.mode {
            ExecMode::Parallel(n) if n >= 1 => self.drive_parallel(inputs),
            _ => self.drive_sequential(inputs),
        }
    }

    /// 单线程 BFS 驱动循环：注入外部 inputs（machine, port, payload）→ process →
    /// 按 `LinkSpec` 把输出路由到下游机器 → 逐级传播，直到没有新输出。
    ///
    /// # 路由语义
    ///
    /// 每个输出值（按端口名匹配 `PhysicalLink`）：
    /// - 命中一条 link → 用 `HasPortInfo::from_port_name` 构造下游输入并
    ///   入队（BFS 逐级传播）；
    /// - 未命中（终端机器 / 观察端口无下游）→ 收集为最终输出返回。
    ///
    /// # LinkKind 物理化（Sequential 模式）
    ///
    /// 单线程顺序驱动下，所有 link kind（Inline/BoundedBuf/Channel…）
    /// 物理化都是**直接 move 投递**：生产者与消费者在同一线程交替执行，
    /// 缓冲永不积压，有界性无物理意义——直接投递是等价物理。
    fn drive_sequential(
        &mut self,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        let topology = self.topology.as_mut().ok_or_else(|| RuntimeError::InitFailed {
            machine: "<none>".into(),
            error: axiom::machine::InitError::Other("runtime not materialized".into()),
        })?;

        let max_ticks = self.config.max_ticks;
        let ids = &topology.ids;
        let topo_order = &topology.topo_order;

        // P0：ID 化队列——(machine_id, port_id, payload)，无 String clone。
        // 外部注入的 (机器名, 端口名) 在入队时一次解析为 ID。
        let mut queue: std::collections::VecDeque<(usize, u16, Box<dyn core::any::Any + Send>)> =
            std::collections::VecDeque::with_capacity(inputs.len());
        for (name, port, payload) in inputs {
            let mid = *topology
                .machine_index
                .get(&name)
                .ok_or_else(|| RuntimeError::DanglingRef { machine: name.clone(), port: port.clone() })?;
            let pid = ids.in_port_names[mid]
                .iter()
                .position(|p| *p == port.as_str())
                .ok_or_else(|| RuntimeError::DanglingRef { machine: name.clone(), port: port.clone() })? as u16;
            queue.push_back((mid, pid, payload));
        }

        let mut outputs: Vec<ProcessResult> = Vec::new();
        let mut ticks: u64 = 0;

        // A1 停机传播：pending_sources = in_degree 的克隆副本（按 topo_order
        // 索引）；stopped = 已停机机器位集（按 ID，O(1) 检查）。
        let mut pending_sources: Vec<usize> = topology.in_degree.clone();
        let machine_index = &topology.machine_index;
        let mut stopped: Vec<bool> = vec![false; topology.machines.len()];

        while let Some((mid, pid, payload)) = queue.pop_front() {
            // 已停机机器：丢弃后续消息（Done 是停机信号，不是 Idle）。
            if stopped[mid] {
                continue;
            }
            ticks += 1;
            if let Some(limit) = max_ticks {
                if ticks > limit {
                    return Err(RuntimeError::TickLimitExceeded { ticks });
                }
            }

            let name = &topo_order[mid];
            let machine = topology.machines.get_mut(name).ok_or_else(|| {
                RuntimeError::DanglingRef { machine: name.clone(), port: String::new() }
            })?;
            let result = machine.inject(pid, payload);

            // Done = 停机信号：本机器停机，并级联停机"所有入边源均已
            // 停机"的下游（显式传播，而非仅忽略）。
            if matches!(result, ProcessResult::Done) {
                mark_stopped(mid, name, &mut stopped, &mut pending_sources, machine_index, &topology.links);
                continue;
            }

            // 路由：输出按端口名找下游；无下游则作为终端输出收集。
            // P0：route_by_src[mid] 线性扫描（输出端口通常 1-2），
            // Yield 的端口名（&'static str）直接比较，无字符串表查找。
            match result {
                ProcessResult::Idle => {}
                ProcessResult::Yield { port, value } => {
                    if let Some((_, dst_mid, dst_pid)) = ids.route_by_src[mid]
                        .iter()
                        .find(|(sp, _, _)| *sp == port)
                    {
                        queue.push_back((*dst_mid, *dst_pid, value));
                    } else {
                        outputs.push(ProcessResult::Yield { port, value });
                    }
                }
                ProcessResult::YieldMulti { outputs: list } => {
                    for (port, value) in list {
                        if let Some((_, dst_mid, dst_pid)) = ids.route_by_src[mid]
                            .iter()
                            .find(|(sp, _, _)| *sp == port)
                        {
                            queue.push_back((*dst_mid, *dst_pid, value));
                        } else {
                            outputs.push(ProcessResult::Yield { port, value });
                        }
                    }
                }
                ProcessResult::Done => unreachable!("handled above"),
            }
        }
        Ok(outputs)
    }

    /// 多线程驱动：每机器一个 OS 线程，链接按 `LinkKind` 物化为真实 channel。
    ///
    /// # 物理载体（按 `LinkKind` 选择，见 [`carrier::channel_for`]）
    ///
    /// - `BoundedBuf { Blocking }` / `Channel { !drop_when_full }` →
    ///   `sync_channel(capacity)` + 阻塞 `send`（自然背压）；
    /// - `BoundedBuf { Dropping }` / `Channel { drop_when_full }` →
    ///   `sync_channel` + `try_send`（满则丢弃新消息）；
    /// - `BoundedBuf { Overwriting }` → **自定义有界覆盖载体**
    ///   （满时覆盖最老——原生语义，非 `try_send` 近似）；
    /// - `Latest` / `SharedState` → **单槽覆盖载体**（读者见最新值）；
    /// - `Inline` / `CasFreeRing` → 无界 `channel`（跨线程 Inline 即
    ///   函数调用→channel 的语义迁移；CasFreeRing 的无锁载体属嵌入式场景）。
    ///
    /// `ReadPolicy::NonBlocking` 已物理化：单入边 + BoundedBuf 时线程
    /// 走 `try_recv` + `yield_now` 轮询（不阻塞线程）。
    ///
    /// # fan-in 支持
    ///
    /// 每目标机器可有多条入边：各入边一个 receiver，机器线程经 forward
    /// 线程合并消费（按到达顺序注入）。fan-in 下 `NonBlocking` 降级为
    /// 阻塞（合并 channel 用阻塞 recv）。
    ///
    /// # 限制
    ///
    /// - 每机器必须有一个输入端口（Source 类无输入机器暂不支持）；
    /// - **有环拓扑**：环中线程无法靠 channel 断开级联停机（互相保活），
    ///   改用全局 `stop_signal`（`Arc<AtomicBool>`）+ 每线程 tick 计数器
    ///   驱动——任何线程达到 `Done` 或 tick 超限即全局停机。无环拓扑
    ///   保持现有 channel 断开级联停机路径。
    ///
    /// # 停机：channel 断开级联
    ///
    /// tick 注入后 drop 所有入口 sender → 入口线程 `recv` 返回 `None` →
    /// 退出 → drop 自己的输出 sender → 下游 `recv` 断开 → 级联停止 →
    /// `thread::scope` 收敛。终端输出经结果 channel 收集。
    fn drive_parallel(
        &mut self,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        use std::sync::mpsc;

        let topology = self.topology.as_mut().ok_or_else(|| RuntimeError::InitFailed {
            machine: "<none>".into(),
            error: axiom::machine::InitError::Other("runtime not materialized".into()),
        })?;
        let ids = &topology.ids;

        // ── 1. 构建 link channel 载体 ────────────────────────────────────
        // A2：支持 fan-in——每目标机器可有多条入边，各入边一个 receiver，
        // 机器线程经 forward 线程合并消费（见下方 spawn 逻辑）。
        // channel 传 (port_name, payload)——port 名随消息走，线程循环用收到的
        // port 名 inject，而非固定端口。这统一了 Sequential/Parallel 的 inject
        // 语义，也为未来多输入端口机器做好准备。

        // 输出路由表：src_machine → (src_port → (dst_port, 下游 carrier))。
        // dst_port 随 link 走——路由时附带在消息里，下游线程用它 inject。
        // 按机器分组，每个机器线程持有**自己那组 sender 的所有权**——
        // 线程退出时 drop → 下游 recv 断开 → 级联停机才能生效。
        // （若集中在函数级变量，线程退出只 drop 引用，下游 sender 仍存活，
        //   下游线程永远阻塞——死锁。）
        let mut out_routes: BTreeMap<
            String,
            BTreeMap<String, (String, ChanSender)>,
        > = BTreeMap::new();
        // 输入接收表：dst_machine → 多条入边的 receiver 列表（A2 fan-in）。
        // 不再存 dst_port——port 名随消息走，receiver 只需收消息。
        let mut in_routes: BTreeMap<String, Vec<ChanReceiver>> = BTreeMap::new();
        // 单入边机器的 read_policy（NonBlocking 轮询用）；fan-in 或无
        // BoundedBuf 入边默认 Blocking。
        let mut in_policies: BTreeMap<String, axiom::link::ReadPolicy> = BTreeMap::new();
        for link in &topology.links {
            // 按 LinkKind 物理化 carrier（见 channel_for 的载体矩阵）。
            let (tx, rx) = channel_for(&link.kind);
            out_routes
                .entry(link.src_machine.clone())
                .or_default()
                .insert(link.src_port.clone(), (link.dst_port.clone(), tx));
            in_routes.entry(link.dst_machine.clone()).or_default().push(rx);
            if let axiom::link::LinkKind::BoundedBuf { read_policy, .. } = &link.kind {
                // 仅单入边时生效（fan-in 由 forward 线程阻塞合并）。
                in_policies.entry(link.dst_machine.clone()).or_insert(*read_policy);
            }
        }

        // 入口机器（无入边）：tick 持有注入 sender。入口 channel 恒为无界
        // （外部注入不应被背压阻塞）。Source 类（无输入端口）无法驱动，
        // 因 inject 无端口可匹配——直接报错。
        //
        // 有环拓扑的特殊处理：环中所有机器都有入边，但外部输入仍需注入。
        // 为被外部 input 引用的机器创建额外的 entry channel——它的
        // receiver 与 link carrier 一起 forward 合并到机器线程。
        let mut entry_txs: BTreeMap<String, mpsc::Sender<RoutedMsg>> = BTreeMap::new();
        let mut entry_rxs: BTreeMap<String, mpsc::Receiver<RoutedMsg>> = BTreeMap::new();
        for (name, machine) in &topology.machines {
            if in_routes.contains_key(name) {
                continue;
            }
            if machine.port_schema().inputs().next().is_none() {
                return Err(RuntimeError::UnsupportedTopology {
                    machine: name.clone(),
                    reason: "machine has no input port (Source-like) is not supported in Parallel mode".into(),
                });
            }
            let (tx, rx) = mpsc::channel::<RoutedMsg>();
            entry_txs.insert(name.clone(), tx);
            entry_rxs.insert(name.clone(), rx);
        }

        // 有环检测：环中线程无法靠 channel 断开级联停机（互相保活），
        // 改用全局 stop_signal + tick 限制驱动。必须在 entry channel 处理
        // 之前算出——有环时需为"已有入边但被外部 input 引用"的机器额外
        // 创建 entry channel（环中所有机器都有入边，否则外部输入无处注入）。
        let cyclic = has_cycle(&topology.topo_order, &topology.links);
        let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let max_ticks = self.config.max_ticks.unwrap_or(1_000_000);

        // 有环拓扑：为被外部 input 引用但已有入边的机器创建 entry channel。
        // 这些机器的线程同时从 link carrier 和 entry channel 消费（forward 合并）。
        if cyclic {
            for (name, _, _) in &inputs {
                if !entry_txs.contains_key(name) && topology.machines.contains_key(name) {
                    let (tx, rx) = mpsc::channel::<RoutedMsg>();
                    entry_txs.insert(name.clone(), tx);
                    // entry rx 直接加入 in_routes（与 link carrier 一起 forward 合并）；
                    // 不放入 entry_rxs——避免双重所有权。
                    in_routes.entry(name.clone()).or_default().push(ChanReceiver::Mpsc(rx));
                }
            }
        }

        // 结果收集 channel（终端输出）。
        let (result_tx, result_rx) = mpsc::channel::<ProcessResult>();

        // ── 2. scoped 线程：每机器一个；注入与级联停机也在 scope 内 ──
        // （线程 spawn 后立即 recv；若注入在 scope 外，scope 会先阻塞
        //   等待线程 join，而线程永远等不到输入——死锁。）
        let machines: Vec<(&String, &mut Box<dyn RunningMachine>)> =
            topology.machines.iter_mut().collect();

        // 注入前验证：所有外部 input 的目标必须是入口机器。
        for (name, _, _) in &inputs {
            if !entry_txs.contains_key(name) {
                return Err(RuntimeError::DanglingRef {
                    machine: name.clone(),
                    port: "<entry>".to_string(),
                });
            }
        }

        std::thread::scope(|s| {
            for (name, machine) in machines {
                // 机器的输入 receiver：入边机器的 in_routes（可多条，A2 fan-in
                // 经 forward 线程合并），否则入口机器的 entry_rxs。
                // port 名随消息走（不在 receiver 侧存）——统一 Sequential/Parallel 的 inject 语义。
                let rx = match in_routes.remove(name) {
                    Some(mut v) if v.len() == 1 => v.pop().expect("len 1"),
                    Some(v) => {
                        // fan-in：每条入边一个 forward 线程 → 合并 channel →
                        // 本线程 recv 合并 rx（按到达顺序）。所有 forward 退出
                        // （上游 sender 断开 / stop_signal）后合并 rx recv
                        // 断开 → 级联停机。
                        let (merge_tx, merge_rx) = mpsc::channel::<RoutedMsg>();
                        let stop_fwd = stop_signal.clone();
                        for rx in v {
                            let merge_tx = merge_tx.clone();
                            let stop_fwd = stop_fwd.clone();
                            s.spawn(move || {
                                if stop_fwd.load(std::sync::atomic::Ordering::Relaxed) {
                                    return;
                                }
                                // 有环时用 try_recv + yield（避免阻塞 forward 线程
                                // 导致 stop_signal 无法传播）；无环时阻塞 recv。
                                if cyclic {
                                    loop {
                                        if stop_fwd.load(std::sync::atomic::Ordering::Relaxed) {
                                            break;
                                        }
                                        match rx.try_recv() {
                                            Ok(Some(msg)) => { let _ = merge_tx.send(msg); }
                                            Ok(None) => std::thread::yield_now(),
                                            Err(()) => break,
                                        }
                                    }
                                } else {
                                    while let Some(msg) = rx.recv() {
                                        let _ = merge_tx.send(msg);
                                    }
                                }
                            });
                        }
                        drop(merge_tx);
                        ChanReceiver::Mpsc(merge_rx)
                    }
                    None => ChanReceiver::Mpsc(
                        entry_rxs.remove(name).expect("entry machine has an entry channel"),
                    ),
                };
                // 本机器输出 sender 的所有权（退出时 drop → 下游级联停机）。
                let my_routes = out_routes.remove(name).unwrap_or_default();
                let result_tx = &result_tx;
                // NonBlocking：单入边 + BoundedBuf read_policy == NonBlocking。
                let non_blocking = in_policies.get(name).copied()
                    == Some(axiom::link::ReadPolicy::NonBlocking);
                let stop = stop_signal.clone();
                // P0：本机器的输入端口名表（String port → port_id 用）。
                let mid = topology.machine_index.get(name).copied().unwrap_or(0);
                let in_names = &ids.in_port_names[mid];

                s.spawn(move || {
                    let handle: &mut Box<dyn RunningMachine> = machine;
                    // 端口名 → 端口 ID（线性扫描，输入端口通常 1-2）。
                    let pid_of = |port: &str| -> u16 {
                        in_names.iter().position(|p| *p == port).unwrap_or(0) as u16
                    };

                    if cyclic {
                        // 有环模式：全局 stop_signal + tick 限制驱动。
                        // 环中线程无法靠 channel 断开停机（互相保活），
                        // 改用 try_recv + yield + tick 计数。任何线程
                        // Done / tick 超限 → set stop_signal → 全局停机。
                        let mut ticks: u64 = 0;
                        loop {
                            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                            match rx.try_recv() {
                                Ok(Some((port, payload))) => {
                                    ticks += 1;
                                    if ticks > max_ticks {
                                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                        break;
                                    }
                                    let result = handle.inject(pid_of(&port), payload);
                                    if matches!(result, ProcessResult::Done) {
                                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                        break;
                                    }
                                    route_parallel_outputs(result, &my_routes, result_tx);
                                }
                                Ok(None) => std::thread::yield_now(),
                                Err(()) => break,
                            }
                        }
                    } else if non_blocking {
                        // ReadPolicy::NonBlocking：try_recv + 让出（轮询调度，
                        // 不阻塞线程）；Err(()) = 断开 → 退出（级联停机）。
                        loop {
                            match rx.try_recv() {
                                Ok(Some((port, payload))) => {
                                    let result = handle.inject(pid_of(&port), payload);
                                    if matches!(result, ProcessResult::Done) {
                                        break;
                                    }
                                    route_parallel_outputs(result, &my_routes, result_tx);
                                }
                                Ok(None) => std::thread::yield_now(),
                                Err(()) => break,
                            }
                        }
                    } else {
                        // 默认（Blocking）：阻塞 recv。
                        while let Some((port, payload)) = rx.recv() {
                            let result = handle.inject(pid_of(&port), payload);
                            // A1：Done = 停机信号——立即退出，不再处理 channel 积压。
                            if matches!(result, ProcessResult::Done) {
                                break;
                            }
                            route_parallel_outputs(result, &my_routes, result_tx);
                        }
                    }
                    // rx 断开 / Done / stop_signal → 线程退出 → my_routes drop
                    // → 下游 recv 断开 → 级联停机（无环）或 stop_signal 传播（有环）。
                });
            }

            // 注入外部 inputs（线程已开始 recv，send 立即被消费）。
            // port 名随消息发送——线程循环用收到的 port 名 inject。
            // 用 get 而非 remove：多个 input 可能注入**同一**入口机器
            // （remove 第一次后第二次即失败——真实 bug，由 http_declarative
            //  验收用例抓到）。统一 drop 在注入循环之后。
            for (name, port, payload) in inputs {
                let tx = entry_txs.get(&name).expect("validated entry");
                let _ = tx.send((port, payload));
            }
            // 释放全部入口 sender：入口线程 recv 断开 → 级联停机 →
            // scope 收敛（所有线程 join）。
            drop(entry_txs);
        });

        // ── 4. 收集终端输出（直到结果 channel 断开）────────────────────
        // 必须 drop result_tx：scope 内各线程只借用了它的引用，顶层 sender
        // 仍在，否则 recv 永远等不到 Err(disconnected)。
        drop(result_tx);
        let mut outputs = Vec::new();
        while let Ok(r) = result_rx.recv() {
            outputs.push(r);
        }
        Ok(outputs)
    }

    /// 清理所有 machine（逆序 cleanup）。
    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if let Some(mut topology) = self.topology.take() {
            while let Some((_, machine)) = topology.machines.pop_last() {
                machine.cleanup()?;
            }
        }
        Ok(())
    }

    // ── IO 多路复用集成 ───────────────────────────────────────────────────
    //
    // 外部注册模型：调用方创建 IO source（如 TcpListener），取 raw fd/socket，
    // 通过 register_io 注册 token→(machine, port) 映射 + reactor 兴趣。
    // run_io poll reactor → 就绪事件转为 inputs → 合并外部 inputs → tick。
    //
    // 保持 Machine::process 签名不变——machine 收到 IoEvent 作为普通
    // 类型化输入（输入端口类型含 `IoEvent` variant），在 process 中执行
    // 实际 IO（read/write/accept）。

    /// 注册一个 IO source 的就绪兴趣 + 路由映射。
    ///
    /// - `token`：调用方分配的令牌，用于关联就绪事件与 machine。
    /// - `machine` / `port`：就绪时注入的目标机器名与输入端口名。
    /// - `raw`：OS 级 fd（Unix）/ socket（Windows）。
    /// - `interest`：READABLE / WRITABLE / READ_WRITE。
    pub fn register_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        token: IoToken,
        machine: &str,
        port: &str,
        raw: RawIo,
        interest: IoInterest,
    ) -> Result<(), RuntimeError> {
        reactor.register(raw, interest, token).map_err(|e| RuntimeError::IoFailed { error: e })?;
        self.io_routing.insert(token, (machine.to_string(), port.to_string()));
        Ok(())
    }

    /// 更新已注册 IO source 的兴趣（readiness 模型下 rearm）。
    pub fn reregister_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        token: IoToken,
        machine: &str,
        port: &str,
        raw: RawIo,
        interest: IoInterest,
    ) -> Result<(), RuntimeError> {
        reactor.reregister(raw, interest, token).map_err(|e| RuntimeError::IoFailed { error: e })?;
        self.io_routing.insert(token, (machine.to_string(), port.to_string()));
        Ok(())
    }

    /// 注销一个 IO source。
    pub fn deregister_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        raw: RawIo,
        token: IoToken,
    ) -> Result<(), RuntimeError> {
        reactor.deregister(raw).map_err(|e| RuntimeError::IoFailed { error: e })?;
        self.io_routing.remove(&token);
        Ok(())
    }

    /// IO 感知的单次驱动：poll reactor → 就绪事件转为 inputs → 合并外部
    /// inputs → 调用现有 tick 循环 → 返回终端输出。
    ///
    /// - `timeout`：传给 reactor poll 的等待上限。`None` = 阻塞直到有事件；
    ///   `Some(0)` = 非阻塞（立即返回当前就绪）。
    /// - 未注册 token 的就绪事件被丢弃（reactor 可能报告已 deregister 的源）。
    pub fn run_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        external_inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
        timeout: Option<core::time::Duration>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        let io_events = reactor
            .poll(timeout)
            .map_err(|e| RuntimeError::IoFailed { error: e })?;

        let mut inputs = external_inputs;
        for event in io_events {
            if let Some((machine, port)) = self.io_routing.get(&event.token) {
                inputs.push((machine.clone(), port.clone(), Box::new(event)));
            }
        }
        self.tick(inputs)
    }
}
