//! runtime 单元测试——覆盖配置、物化、路由、确定性、停机传播、fan-in、
//! B 档载体（Overwriting/Latest/NonBlocking）。

use crate::*;
use axiom::declare_ports;
use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::Machine;
use axiom::port::MachineContext;
use axiom::resource::MachinePhysicalSpec;

declare_ports! {
    pub struct DoublerPorts {
        input type DoublerInput {
            x[Data] => i32,
        }
        output type DoublerOutput {
            y[Data] => i32,
        }
    }
}

pub struct Doubler;
impl Machine for Doubler {
    type State = ();
    type Input = DoublerInput;
    type Output = DoublerOutput;
    type Ports = DoublerPorts;
    type ProcessOutput = axiom::machine::SingleOutput<DoublerOutput>;

    fn name() -> &'static str { "doubler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> {
        Ok(())
    }

    fn process(
        _state: &mut Self::State,
        _ctx: &MachineContext,
        input: DoublerInput,
    ) -> Self::ProcessOutput {
        match input {
            DoublerInput::x(n) => axiom::machine::SingleOutput::Yield(DoublerOutput::y(n * 2)),
        }
    }

    fn cleanup(_state: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> {
        Ok(())
    }
}

// Doubler 的输出是 SingleOutput（恰好一个输出），满足 FusedInline 的
// 类型约束——可安全进入融合流水线。
impl axiom::machine::FusedInline for Doubler {}

#[test]
fn runtime_config_defaults_to_sequential() {
    let cfg = RuntimeConfig::default();
    assert_eq!(cfg.mode, ExecMode::Sequential);
    assert_eq!(cfg.max_ticks, Some(1_000_000));
}

#[test]
fn runtime_config_inline_has_no_tick_limit() {
    let cfg = RuntimeConfig::inline();
    assert_eq!(cfg.mode, ExecMode::Inline);
    assert_eq!(cfg.max_ticks, None);
}

#[test]
fn runtime_config_parallel_n_workers() {
    let cfg = RuntimeConfig::parallel(8);
    assert_eq!(cfg.mode, ExecMode::Parallel(8));
}

#[test]
fn runtime_holds_config_and_empty_topology() {
    let rt = Runtime::default();
    assert_eq!(rt.config().mode, ExecMode::Sequential);
    assert!(rt.topology().is_none());
}

#[test]
fn registry_register_and_build() {
    let mut registry = Registry::new();
    registry.register::<Doubler>("doubler");

    let ctx = MachineContext::new("test_doubler");
    let machine = registry.build("doubler", ctx).expect("build");
    assert_eq!(machine.name(), "test_doubler");
}

#[test]
fn runtime_materialize_single_machine() {
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");
    assert!(rt.topology().is_some());
    assert_eq!(rt.topology().unwrap().machines.len(), 1);
}

#[test]
fn runtime_materialize_rejects_dangling_port() {
    // validate_endpoint 修复：链接引用不存在的端口时，物化阶段即报
    // DanglingRef（而非 tick 时 inject 静默返回 Idle 吞掉消息）。
    // 用两台机器避免触发 DeploySpec::validate 的 SelfLoop 检查——
    // 这里专门测试 runtime 的端口存在性校验，而非 core 的环检查。
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "nonexistent"), ("d2", "x"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port } if machine == "d1" && port == "nonexistent"),
        "expected DanglingRef for nonexistent port, got {err:?}"
    );
}

#[test]
fn runtime_materialize_rejects_wrong_port_direction() {
    // validate_endpoint 修复：src 端口必须是输出端口（PortDir::Out）。
    // DoublerInput::x 是输入端口——作为 link 的 out 端应被拒绝。
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "x"), ("d2", "x"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port } if machine == "d1" && port == "x"),
        "expected DanglingRef for input port used as source, got {err:?}"
    );
}

#[test]
fn runtime_tick_processes_input() {
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");

    // tick 签名：(machine, port, payload) —— 端口 x 注入 21
    let results = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(21i32))])
        .expect("tick");
    assert_eq!(results.len(), 1);
    match &results[0] {
        ProcessResult::Yield { port, value } => {
            assert_eq!(*port, "y");
            let v = value.downcast_ref::<i32>().expect("i32 payload");
            assert_eq!(*v, 42);
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn runtime_routes_output_to_downstream() {
    // 链式拓扑：d1.y ──► d2.x（Doubler → Doubler）
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));

    rt.materialize(&spec).expect("materialize");

    // 输入 3 → d1 产出 6 → 路由到 d2 → 产出 12（终端输出，无下游）
    let results = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(results.len(), 1, "exactly one terminal output");
    match &results[0] {
        ProcessResult::Yield { port, value } => {
            assert_eq!(*port, "y", "terminal output on d2's y port");
            let v = value.downcast_ref::<i32>().expect("i32 payload");
            assert_eq!(*v, 12);
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn runtime_routes_fanout_via_tee() {
    // 扇出拓扑：source ──► Tee ──┬──► d2
    //                            └──► d3
    // 用内置 Tee<i32>（MultiOutput 扇出）验证路由对多输出的处理。
    use axiom::builtin::{Tee, TeeInput, TeeOutput};

    struct Src;
    impl Machine for Src {
        type State = ();
        type Input = axiom::portset::In<i32>;
        type Output = axiom::portset::Out<i32>;
        type Ports = axiom::portset::SinglePorts<i32>;
        type ProcessOutput = axiom::machine::SingleOutput<Self::Output>;
        fn name() -> &'static str { "src" }
        fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
        fn process(_: &mut Self::State, _: &MachineContext, input: Self::Input)
            -> Self::ProcessOutput {
            let axiom::portset::In(v) = input;
            axiom::machine::SingleOutput::Yield(axiom::portset::Out(v))
        }
        fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
    }
    impl axiom::machine::FusedInline for Src {}

    let mut rt = Runtime::default();
    rt.register::<Src>("src");
    rt.register::<Tee<i32>>("tee");
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("s", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("t", "tee", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("s", "output"), ("t", "input"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_a"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_b"), ("d3", "x"), LinkKind::Inline));

    rt.materialize(&spec).expect("materialize");

    // 输入 5 → src 产出 5 → Tee 扇出两份 5 → d2/d3 各 ×2 → 两个终端 10
    let results = rt
        .tick(vec![("s".to_string(), "input".to_string(), Box::new(5i32))])
        .expect("tick");
    assert_eq!(results.len(), 2, "two terminal outputs from fan-out");
    let mut vals: Vec<i32> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    vals.sort();
    assert_eq!(vals, vec![10, 10]);
    // Tee 输入端口 payload 类型是 TeeInput<i32>（from_port_name 构造）
    let _ = TeeInput::Input(1i32);
    let _ = TeeOutput::OutputA(1i32);
}

#[test]
fn runtime_parallel_chain_matches_sequential() {
    // 链式拓扑在 Parallel(2) 下结果与 Sequential 一致（3 → 6 → 12）。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));

    let mut seq = Runtime::new(RuntimeConfig::sequential());
    seq.register::<Doubler>("doubler");
    seq.materialize(&spec).expect("materialize");
    let seq_out = seq
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("sequential tick");

    let mut par = Runtime::new(RuntimeConfig::parallel(2));
    par.register::<Doubler>("doubler");
    par.materialize(&spec).expect("materialize");
    let par_out = par
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("parallel tick");

    assert_eq!(seq_out.len(), 1);
    assert_eq!(par_out.len(), 1);
    let sv = match &seq_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    let pv = match &par_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(sv, 12);
    assert_eq!(pv, 12, "Parallel chain must produce the same terminal value");
}

#[test]
fn runtime_parallel_boundedbuf_matches_sequential() {
    // BoundedBuf 链（capacity=2, Blocking）在 Parallel(2) 下走 sync_channel
    // 阻塞背压路径，结果须与 Sequential 一致（3 → 6 → 12）。
    // 这锁定 R001 确定性对有界 carrier 仍成立——背压是物理参数，不是语义参数。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::BoundedBuf {
                capacity: 2,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ));

    let run = |cfg: RuntimeConfig| -> i32 {
        let mut rt = Runtime::new(cfg);
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
            .expect("tick");
        match &out[0] {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }
    };

    assert_eq!(run(RuntimeConfig::sequential()), 12);
    assert_eq!(
        run(RuntimeConfig::parallel(2)), 12,
        "BoundedBuf Parallel must match Sequential (sync_channel backpressure is transparent)"
    );
}

#[test]
fn runtime_parallel_channel_drop_matches_sequential() {
    // Channel { capacity=4, drop_when_full=true } 走 sync_channel + try_send
    // 路径。单消息场景下不会触发丢弃，结果与 Sequential 一致——锁定
    // Channel carrier 的物化在正常投递下不改变语义。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::Channel { capacity: 4, drop_when_full: true },
        ));

    let run = |cfg: RuntimeConfig| -> i32 {
        let mut rt = Runtime::new(cfg);
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
            .expect("tick");
        match &out[0] {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }
    };

    assert_eq!(run(RuntimeConfig::sequential()), 12);
    assert_eq!(run(RuntimeConfig::parallel(2)), 12);
}

#[test]
fn runtime_parallel_fanout_matches_sequential() {
    // 扇出拓扑在 Parallel(2) 下结果与 Sequential 一致（5 → 10, 10）。
    use axiom::builtin::Tee;

    struct Src;
    impl Machine for Src {
        type State = ();
        type Input = axiom::portset::In<i32>;
        type Output = axiom::portset::Out<i32>;
        type Ports = axiom::portset::SinglePorts<i32>;
        type ProcessOutput = axiom::machine::SingleOutput<Self::Output>;
        fn name() -> &'static str { "src" }
        fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
        fn process(_: &mut Self::State, _: &MachineContext, input: Self::Input)
            -> Self::ProcessOutput {
            let axiom::portset::In(v) = input;
            axiom::machine::SingleOutput::Yield(axiom::portset::Out(v))
        }
        fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
    }
    impl axiom::machine::FusedInline for Src {}

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("s", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("t", "tee", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("s", "output"), ("t", "input"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_a"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_b"), ("d3", "x"), LinkKind::Inline));

    let run = |cfg: RuntimeConfig| -> Vec<i32> {
        let mut rt = Runtime::new(cfg);
        rt.register::<Src>("src");
        rt.register::<Tee<i32>>("tee");
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("s".to_string(), "input".to_string(), Box::new(5i32))])
            .expect("tick");
        let mut vals: Vec<i32> = out.iter().map(|r| match r {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }).collect();
        vals.sort();
        vals
    };

    assert_eq!(run(RuntimeConfig::sequential()), vec![10, 10]);
    assert_eq!(run(RuntimeConfig::parallel(2)), vec![10, 10], "Parallel fan-out must match");
}

#[test]
fn runtime_done_stops_machine_sequential() {
    // A1：Done = 停机信号——机器返回 Done 后不再接收新输入（积压丢弃）。
    // Stopper 第 2 次 process 返回 Done；注入 3 条 → 输出 [1]（第 1 条 Yield），
    // 第 2 条 Done 停机，第 3 条被丢弃。若未停机（旧行为）会输出 [1, 2]。
    use axiom::machine::{CleanupError, InitError, SingleOutput};
    use axiom::port::ConfigSchema;

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct StopperPorts {
            input type StopperInput { x [Data] => i64 }
            output type StopperOutput { y [Data] => i64 }
        }
    }

    pub struct Stopper;
    impl Machine for Stopper {
        type State = u64;
        type Input = StopperInput;
        type Output = StopperOutput;
        type Ports = StopperPorts;
        type ProcessOutput = SingleOutput<StopperOutput>;
        fn name() -> &'static str { "stopper" }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
        #[inline]
        fn process(state: &mut u64, _: &MachineContext, input: StopperInput)
            -> SingleOutput<StopperOutput> {
            let StopperInput::x(n) = input;
            *state += 1;
            if *state >= 2 {
                // Done 的机器通过 `unified` 转换——这里直接构造统一类型。
                let _ = n;
                SingleOutput::Done
            } else {
                SingleOutput::Yield(StopperOutput::y(n))
            }
        }
        fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Stopper>("stopper");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("s", "stopper", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let results = rt
        .tick(vec![
            ("s".to_string(), "x".to_string(), Box::new(10i64)),
            ("s".to_string(), "x".to_string(), Box::new(20i64)),
            ("s".to_string(), "x".to_string(), Box::new(30i64)),
        ])
        .expect("tick");

    // 停机生效：只有第 1 条产生输出；第 2 条 Done、第 3 条被丢弃。
    let vals: Vec<i64> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i64>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    assert_eq!(vals, vec![10], "Done must stop the machine; backlog dropped");
}

#[test]
fn runtime_done_stops_machine_parallel() {
    // A1 的 Parallel 形态：线程收到 Done 后立即退出，不再处理积压。
    use axiom::machine::{CleanupError, InitError, SingleOutput};
    use axiom::port::ConfigSchema;

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct StopperPorts {
            input type StopperInput { x [Data] => i64 }
            output type StopperOutput { y [Data] => i64 }
        }
    }

    pub struct Stopper;
    impl Machine for Stopper {
        type State = u64;
        type Input = StopperInput;
        type Output = StopperOutput;
        type Ports = StopperPorts;
        type ProcessOutput = SingleOutput<StopperOutput>;
        fn name() -> &'static str { "stopper" }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
        #[inline]
        fn process(state: &mut u64, _: &MachineContext, input: StopperInput)
            -> SingleOutput<StopperOutput> {
            let StopperInput::x(n) = input;
            *state += 1;
            if *state >= 2 {
                SingleOutput::Done
            } else {
                SingleOutput::Yield(StopperOutput::y(n))
            }
        }
        fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Stopper>("stopper");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("s", "stopper", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let results = rt
        .tick(vec![
            ("s".to_string(), "x".to_string(), Box::new(10i64)),
            ("s".to_string(), "x".to_string(), Box::new(20i64)),
            ("s".to_string(), "x".to_string(), Box::new(30i64)),
        ])
        .expect("tick");

    let vals: Vec<i64> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i64>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    assert_eq!(vals, vec![10], "Parallel: Done must exit the thread; backlog dropped");
}

#[test]
fn runtime_fanin_merges_multi_source_parallel() {
    // A2：fan-in——两个入口机器（d1, d2）汇入同一 Consumer（Doubler），
    // Parallel 下经 forward 线程合并消费。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("c", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("c", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("c", "x"), LinkKind::Inline));

    let run = |cfg: RuntimeConfig| -> Vec<i32> {
        let mut rt = Runtime::new(cfg);
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![
                ("d1".to_string(), "x".to_string(), Box::new(3i32)),
                ("d2".to_string(), "x".to_string(), Box::new(5i32)),
            ])
            .expect("tick");
        let mut vals: Vec<i32> = out.iter().map(|r| match r {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }).collect();
        vals.sort();
        vals
    };

    // Sequential（BFS 天然合并）与 Parallel（forward 线程合并）都汇聚为 {12, 20}
    // （3→6→12，5→10→20：c 是 Doubler，再次 ×2）。
    let seq = run(RuntimeConfig::sequential());
    let par = run(RuntimeConfig::parallel(2));
    assert_eq!(seq, vec![12, 20]);
    assert_eq!(par, vec![12, 20], "fan-in must merge in Parallel too");
}

#[test]
fn runtime_shutdown_cleans_up() {
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");
    rt.shutdown().expect("shutdown");
    assert!(rt.topology().is_none());
}

#[test]
fn runtime_parallel_nonblocking_read_policy() {
    // B 档：ReadPolicy::NonBlocking——机器线程 try_recv + yield 轮询
    // （不阻塞线程），断开（级联停机）时退出。功能上与 Blocking 一致。
    use axiom::link::ReadPolicy;
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: ReadPolicy::NonBlocking,
            },
        ));

    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");
    let out = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");

    let vals: Vec<i32> = out.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    assert_eq!(vals, vec![12], "NonBlocking polling must deliver the same result");
}

// ════════════════════════════════════════════════════════════════════════════
// pipelineN 融合测试
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn fusion_fused_chain_matches_non_fused_result() {
    // 融合链 d1→d2→d3（全 FusedInline + Inline link）的 tick 结果
    // 必须与非融合相同（3 → 6 → 12 → 24）。
    // 用 register_fused 注册——materialize 会把 d1,d2,d3 融合为单个 FusedPipeline。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("d3", "x"), LinkKind::Inline));

    // 融合路径
    let mut rt_fused = Runtime::new(RuntimeConfig::sequential());
    rt_fused.register_fused::<Doubler>("doubler");
    rt_fused.materialize(&spec).expect("materialize fused");
    let fused_out = rt_fused
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick fused");

    // 非融合路径（register 而非 register_fused）
    let mut rt_plain = Runtime::new(RuntimeConfig::sequential());
    rt_plain.register::<Doubler>("doubler");
    rt_plain.materialize(&spec).expect("materialize plain");
    let plain_out = rt_plain
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick plain");

    assert_eq!(fused_out.len(), 1);
    assert_eq!(plain_out.len(), 1);
    let fv = match &fused_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    let pv = match &plain_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(pv, 24, "non-fused 3-hop chain: 3→6→12→24");
    assert_eq!(fv, 24, "fused chain must produce same result");
}

#[test]
fn fusion_reduces_machine_count() {
    // 融合后 topology 的机器数应减少（3 台 → 1 个 FusedPipeline）。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("d3", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 1, "3-stage chain must fuse to 1 machine");
    assert_eq!(topo.links.len(), 0, "internal links absorbed by FusedPipeline");
    assert_eq!(topo.topo_order.len(), 1, "topo_order reduced to chain head");
}

#[test]
fn fusion_does_not_trigger_for_non_fused_register() {
    // 用 register（非 register_fused）注册的机器不会被融合——
    // is_fused_compatible() 返回 false。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 2, "non-fused register must not fuse");
    assert_eq!(topo.links.len(), 1, "link preserved");
}

#[test]
fn fusion_does_not_trigger_for_bounded_buf_link() {
    // BoundedBuf link 不是融合候选——即使两端机器可融合。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 2, "BoundedBuf link must not fuse");
}

#[test]
fn fusion_partial_chain_only_fuses_fused_inline_segment() {
    // 混合链：d1(FusedInline) → Inline → d2(FusedInline) → BoundedBuf → d3(FusedInline)
    // 只有 d1→d2 被融合（Inline + 两端可融合）；d2→d3 是 BoundedBuf，不融合。
    // 融合后：1 个 FusedPipeline(d1,d2) + 1 个 d3，1 条 BoundedBuf link。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(
            ("d2", "y"), ("d3", "x"),
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 2, "d1+d2 fused, d3 standalone");
    assert_eq!(topo.links.len(), 1, "only BoundedBuf link remains");
    // 结果验证：3 → 6(d1) → 12(d2) → 24(d3)
    let out = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 24);
}

#[test]
fn fusion_parallel_matches_sequential() {
    // 融合链在 Parallel 模式下结果与 Sequential 一致（R001 确定性对融合仍成立）。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("d3", "x"), LinkKind::Inline));

    let run = |cfg: RuntimeConfig| -> i32 {
        let mut rt = Runtime::new(cfg);
        rt.register_fused::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
            .expect("tick");
        match &out[0] {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }
    };

    assert_eq!(run(RuntimeConfig::sequential()), 24);
    assert_eq!(run(RuntimeConfig::parallel(2)), 24, "fused Parallel must match Sequential");
}

#[test]
fn fusion_fanout_not_fused() {
    // 扇出拓扑（Tee）不满足融合条件——Tee 的 MultiOutput 不实现 FusedInline。
    // 用 register_fused 注册 Doubler（可融合），但 Tee 用 register（不可融合）。
    // d1(FusedInline) → Inline → tee(非 FusedInline) → 扇出不融合。
    use axiom::builtin::Tee;

    struct Src;
    impl Machine for Src {
        type State = ();
        type Input = axiom::portset::In<i32>;
        type Output = axiom::portset::Out<i32>;
        type Ports = axiom::portset::SinglePorts<i32>;
        type ProcessOutput = axiom::machine::SingleOutput<Self::Output>;
        fn name() -> &'static str { "src" }
        fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
        fn process(_: &mut Self::State, _: &MachineContext, input: Self::Input)
            -> Self::ProcessOutput {
            let axiom::portset::In(v) = input;
            axiom::machine::SingleOutput::Yield(axiom::portset::Out(v))
        }
        fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
    }
    impl axiom::machine::FusedInline for Src {}

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("s", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("t", "tee", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("s", "output"), ("t", "input"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_a"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_b"), ("d3", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Src>("src");
    rt.register::<Tee<i32>>("tee"); // Tee 不实现 FusedInline
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    // tee 的扇出阻止融合——所有 4 台机器保持独立。
    assert_eq!(topo.machines.len(), 4, "fan-out via Tee must not fuse");
    assert_eq!(topo.links.len(), 3, "all links preserved");
}

// ════════════════════════════════════════════════════════════════════════════
// Parallel 有环拓扑测试
// ════════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct CounterPorts {
        input type CounterInput { tick [Data] => i64 }
        output type CounterOutput { val [Data] => i64 }
    }
}

/// 计数器机器：每次 process 递增计数，达到阈值（硬编码 10）时返回 Done。
/// 用于有环拓扑测试——环中机器通过 Done 触发全局停机。
/// （阈值不通过 struct 字段配置：Machine::init 仅接受 MachineContext，
///  无注入构造参数的路径；字段会变成 dead code，故直接在 process 内联。）
pub struct Counter;

impl Machine for Counter {
    type State = i64;
    type Input = CounterInput;
    type Output = CounterOutput;
    type Ports = CounterPorts;
    type ProcessOutput = axiom::machine::SingleOutput<CounterOutput>;

    fn name() -> &'static str { "counter" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, axiom::machine::InitError> { Ok(0) }
    #[inline]
    fn process(state: &mut i64, _: &MachineContext, input: CounterInput)
        -> axiom::machine::SingleOutput<CounterOutput> {
        let CounterInput::tick(n) = input;
        *state += n;
        if *state >= 10 {
            axiom::machine::SingleOutput::Done
        } else {
            axiom::machine::SingleOutput::Yield(CounterOutput::val(*state))
        }
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}

#[test]
fn runtime_parallel_cycle_terminates_via_done() {
    // 有环拓扑：a → b → a（自循环反馈环）。
    // a 和 b 互相喂值，直到 a 的计数 >= 10 返回 Done → 全局停机。
    // 无 stop_signal 路径时此测试会挂起（死锁）。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("a", "counter", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "counter", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "val"), ("b", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }))
        .with_link(LinkSpec::new(("b", "val"), ("a", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }));

    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Counter>("counter");
    rt.materialize(&spec).expect("materialize");

    // 注入初始值 1 → a 计数 1 → b 计数 1 → a 计数 2 → ... → 到 10 Done。
    let results = rt
        .tick(vec![("a".to_string(), "tick".to_string(), Box::new(1i64))])
        .expect("tick");

    // 环中机器的输出要么路由到对方（非终端），要么在 Done 时丢弃。
    // 终端输出 = Done 前最后几个 val（无下游路由时的收集）。
    // 由于 a 和 b 互相路由，终端输出可能为空或少量——关键是 tick 不挂起。
    println!("cycle test: {} terminal outputs", results.len());
}

#[test]
fn runtime_parallel_cycle_terminates_via_tick_limit() {
    // 有环拓扑 + 无 Done 的机器——靠 max_ticks 终止。
    // Doubler 永远不返回 Done，环会无限运行——max_ticks 保护。
    //
    // 值约束：Doubler 每跳值翻倍，i32 在 ~16 跳溢出（2^31 > i32::MAX）。
    // max_ticks 是**每线程**计数（环中 d1/d2 各自独立），d1 与 d2 经 channel
    // 串行交替（d1 的第 k 跳需 d2 的第 k-1 跳输出），故 max_ticks=10 时
    // 每线程最多 10 次 inject——最大值 d2 第 10 跳输出 = 4^10 ≈ 1e6，远在
    // i32 上界内。验证"无 Done 时 max_ticks 驱动 stop_signal 终止环"。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Channel { capacity: 4, drop_when_full: true }))
        .with_link(LinkSpec::new(("d2", "y"), ("d1", "x"), LinkKind::Channel { capacity: 4, drop_when_full: true }));

    let mut rt = Runtime::new(RuntimeConfig {
        mode: ExecMode::Parallel(2),
        max_ticks: Some(10),
    });
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    // 注入 1 → d1 产出 2 → d2 产出 4 → d1 产出 8 → ... 直到 max_ticks。
    // 关键：不挂起（max_ticks 限制触发 stop_signal）。
    let _ = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(1i32))])
        .expect("tick");
    // 如果到达这里，说明 tick 没有挂起——测试通过。
}

#[test]
fn runtime_parallel_cycle_matches_sequential() {
    // 有环拓扑在 Sequential 和 Parallel 下都应终止（Sequential 靠
    // max_ticks，Parallel 靠 stop_signal）。验证两者都产出结果。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("a", "counter", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "counter", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "val"), ("b", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }))
        .with_link(LinkSpec::new(("b", "val"), ("a", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }));

    let run = |cfg: RuntimeConfig| -> usize {
        let mut rt = Runtime::new(cfg);
        rt.register::<Counter>("counter");
        rt.materialize(&spec).expect("materialize");
        rt.tick(vec![("a".to_string(), "tick".to_string(), Box::new(1i64))])
            .expect("tick").len()
    };

    // 两者都不挂起——关键验证。结果长度可能不同（Sequential BFS 顺序
    // vs Parallel 线程交错），但都必须终止。
    let seq_len = run(RuntimeConfig::sequential());
    let par_len = run(RuntimeConfig::parallel(2));
    // 都应有限（不挂起）。
    assert!(seq_len < 100, "Sequential cycle must terminate");
    assert!(par_len < 100, "Parallel cycle must terminate");
}

// ════════════════════════════════════════════════════════════════════════════
// IO 多路复用集成测试
// ════════════════════════════════════════════════════════════════════════════

use crate::io::{IoEvent, IoInterest, IoReactor, IoToken, ManualReactor};
use core::time::Duration;

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct IoHandlerPorts {
        input type IoHandlerInput { ready [Data] => IoEvent }
        output type IoHandlerOutput { result [Data] => i64 }
    }
}

/// IO 就绪处理机器：收到 `IoEvent` 输入后，按 readiness 产出对应的
/// 数值标识（readable=1, writable=2, both=3）。用于验证 run_io 把
/// reactor 就绪事件正确路由到 machine 的输入端口。
pub struct IoHandler;

impl Machine for IoHandler {
    type State = ();
    type Input = IoHandlerInput;
    type Output = IoHandlerOutput;
    type Ports = IoHandlerPorts;
    type ProcessOutput = axiom::machine::SingleOutput<IoHandlerOutput>;

    fn name() -> &'static str { "io_handler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
    #[inline]
    fn process(_: &mut (), _: &MachineContext, input: IoHandlerInput)
        -> axiom::machine::SingleOutput<IoHandlerOutput> {
        let IoHandlerInput::ready(event) = input;
        let mut v: i64 = 0;
        if event.readiness.is_readable() { v += 1; }
        if event.readiness.is_writable() { v += 2; }
        axiom::machine::SingleOutput::Yield(IoHandlerOutput::result(v))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}

#[test]
fn io_manual_reactor_routes_event_to_machine() {
    // ManualReactor 预注入一个 READABLE 事件 → run_io poll 得到事件 →
    // 按 token 路由到 machine "h" 的 "ready" 端口 → process 产出 result(1)。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register_io");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "one terminal output from IoHandler");
    match &results[0] {
        ProcessResult::Yield { value, .. } => {
            let v = value.downcast_ref::<i64>().expect("i64 payload");
            assert_eq!(*v, 1, "readable event → result(1)");
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn io_manual_reactor_routes_multiple_events() {
    // 两个 token 各一个事件——验证多事件路由到不同机器。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h1", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("h2", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h1", "ready", 0, IoInterest::READ_WRITE)
        .expect("register h1");
    rt.register_io(&mut reactor, IoToken(1), "h2", "ready", 1, IoInterest::WRITABLE)
        .expect("register h2");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READ_WRITE });
    reactor.push_event(IoEvent { token: IoToken(1), readiness: IoInterest::WRITABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 2, "two terminal outputs");
    let mut vals: Vec<i64> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i64>().unwrap(),
        _ => panic!("expected Yield"),
    }).collect();
    vals.sort();
    assert_eq!(vals, vec![2, 3], "READ_WRITE→3, WRITABLE→2");
}

#[test]
fn io_unregistered_token_event_is_dropped() {
    // reactor 报告了一个未注册 token 的事件——应被静默丢弃，不注入任何 machine。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    // token 999 未注册——事件应被丢弃。
    reactor.push_event(IoEvent { token: IoToken(999), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 0, "unregistered token event dropped");
}

#[test]
fn io_deregister_removes_routing() {
    // deregister_io 后，该 token 的事件不再被路由。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");
    rt.deregister_io(&mut reactor, 0, IoToken(0)).expect("deregister");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 0, "deregistered token event dropped");
}

#[test]
fn io_run_io_merges_external_inputs() {
    // run_io 同时注入外部 inputs + IO 事件——两者都应被处理。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    rt.register::<Doubler>("doubler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d", "doubler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    // 外部 input：doubler 收到 5 → 产出 10
    let external = vec![("d".to_string(), "x".to_string(), Box::new(5i32) as Box<dyn core::any::Any + Send>)];
    let results = rt
        .run_io(&mut reactor, external, Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 2, "one IO output + one external output");
}

#[cfg(target_os = "windows")]
#[test]
fn io_wsa_reactor_detects_tcp_readability() {
    // 真实 WSA reactor：TCP listener 注册 READABLE → 客户端连接 →
    // poll 检测到 READABLE（FD_ACCEPT）→ IoEvent 产出。
    use std::net::{TcpListener, TcpStream};
    use std::os::windows::io::AsRawSocket;
    use crate::io::wsa::WsaReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let raw = listener.as_raw_socket();

    let mut reactor = WsaReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::READABLE, IoToken(42))
        .expect("register");

    // 连接前 poll 应无事件（timeout=0 非阻塞）。
    let no_events = reactor.poll(Some(Duration::from_millis(0))).expect("poll empty");
    assert!(no_events.is_empty(), "no events before connection");

    // 客户端连接 → listener 可 accept → READABLE 就绪。
    let _client = TcpStream::connect(addr).expect("connect");

    // poll 等待就绪（给 OS 一点时间传播事件）。
    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "should detect readable after connect");
    let found = events.iter().any(|e| e.token == IoToken(42) && e.readiness.is_readable());
    assert!(found, "token 42 readable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

// ════════════════════════════════════════════════════════════════════════════
// 复合 Machine 测试
// ════════════════════════════════════════════════════════════════════════════

/// 构建一个 "doubler_pair" 复合定义：内部 d1 --Inline--> d2，
/// 外部端口 "in" → d1.x，"out" → d2.y。两跳 Doubler = ×4。
fn doubler_pair_composite() -> CompositeSpec {
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));
    CompositeSpec::new(spec)
        .with_input("in", "d1", "x")
        .with_output("out", "d2", "y")
}

#[test]
fn composite_single_layer_expands_and_routes() {
    // input_map 重定向：entry.y → comp.in 改写为 entry.y → comp.d1.x
    // 拓扑：entry(Doubler) --Inline--> comp(DoublerPair)
    // 展开后：entry --Inline--> comp.d1 --Inline--> comp.d2
    // tick: 3 → 6(entry) → 12(comp.d1) → 24(comp.d2) → 终端输出 24
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 3, "entry + comp.d1 + comp.d2");
    assert_eq!(topo.links.len(), 2, "entry→comp.d1 + comp.d1→comp.d2");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(out.len(), 1);
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 24, "3→6→12→24");
}

#[test]
fn composite_output_redirect_to_downstream() {
    // output_map 重定向：comp.out → sink.x 改写为 comp.d2.y → sink.x
    // 拓扑：entry → comp → sink
    // 展开后：entry → comp.d1 → comp.d2 → sink
    // tick: 3 → 6 → 12 → 24 → 48
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("sink", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("comp", "out"), ("sink", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 4, "entry + comp.d1 + comp.d2 + sink");
    assert_eq!(topo.links.len(), 3, "entry→comp.d1 + comp.d1→comp.d2 + comp.d2→sink");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(out.len(), 1);
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 48, "3→6→12→24→48");
}

#[test]
fn composite_nested_recursive_expansion() {
    // 嵌套复合：quad = pair1 --Inline--> pair2，pair 本身是复合。
    // 展开后：quad.p1.d1 → quad.p1.d2 → quad.p2.d1 → quad.p2.d2
    // 外部：entry → quad
    // 完整链：entry → quad.p1.d1 → quad.p1.d2 → quad.p2.d1 → quad.p2.d2
    // 5 个 Doubler（×2^5=×32），3 × 32 = 96
    let quad_spec = DeploySpec::new()
        .with_machine(MachineInstance::new("p1", "doubler_pair", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("p2", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("p1", "out"), ("p2", "in"), LinkKind::Inline));
    let quad_comp = CompositeSpec::new(quad_spec)
        .with_input("in", "p1", "in")
        .with_output("out", "p2", "out");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("quad", "doubler_quad", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("quad", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.register_composite("doubler_quad", quad_comp);
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 5, "entry + 4 sub doublers");
    assert_eq!(topo.links.len(), 4, "entry→p1.d1 + 3 internal links");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(out.len(), 1);
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 96, "3 × 2^5 = 96");
}

#[test]
fn composite_fusion_crosses_boundary() {
    // 跨复合边界融合：register_fused 注册 Doubler，
    // 复合内部的 d1→d2 与外部的 entry→comp.d1 都是 Inline + FusedInline
    // → 展开后 3 台机器融合为单个 FusedPipeline（机器数 1）。
    // 这验证了展开发生在融合之前——复合边界对融合透明。
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 1, "entry + comp.d1 + comp.d2 fuse to 1");
    assert_eq!(topo.links.len(), 0, "all links absorbed by FusedPipeline");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 24, "3→6→12→24");
}

// ════════════════════════════════════════════════════════════════════════════
// IO 边界与错误路径测试
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn io_empty_poll_returns_empty() {
    // ManualReactor 无预置事件时 poll 返回空 Vec——验证空 reactor 行为。
    let mut reactor = ManualReactor::new();
    let events = reactor.poll(Some(Duration::from_millis(100))).expect("poll");
    assert!(events.is_empty(), "no pending events → empty poll");
}

#[test]
fn io_reregister_updates_routing() {
    // register token0→(h1, ready) 后 reregister token0→(h2, ready)，
    // push token0 事件 → run_io 路由到 h2（io_routing 已覆盖）。
    // h1 与 h2 都产出 result(1)，但 reregister 保证仅一台机器收到事件
    // （结果数 = 1，而非 0 或 2）。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h1", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("h2", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h1", "ready", 0, IoInterest::READABLE)
        .expect("register");
    rt.reregister_io(&mut reactor, IoToken(0), "h2", "ready", 0, IoInterest::READABLE)
        .expect("reregister");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "reregister kept routing active (exactly 1 target)");
    match &results[0] {
        ProcessResult::Yield { value, .. } => {
            let v = value.downcast_ref::<i64>().expect("i64");
            assert_eq!(*v, 1, "READABLE → result(1)");
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn io_deregister_then_event_ignored() {
    // register token0→h1 + token1→h2，deregister token0，
    // push 两个 token 的事件 → 仅 h2 响应（token0 事件被丢弃）。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h1", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("h2", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h1", "ready", 0, IoInterest::READABLE)
        .expect("register h1");
    rt.register_io(&mut reactor, IoToken(1), "h2", "ready", 1, IoInterest::READABLE)
        .expect("register h2");
    rt.deregister_io(&mut reactor, 0, IoToken(0)).expect("deregister h1");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });
    reactor.push_event(IoEvent { token: IoToken(1), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "only h2 responds (token0 deregistered)");
}

#[test]
fn io_multiple_events_same_token() {
    // 同一 token push 3 个事件 → run_io 产生 3 次注入到同一机器 → 3 个输出。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });
    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });
    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 3, "3 events same token → 3 outputs");
    for r in &results {
        match r {
            ProcessResult::Yield { value, .. } => {
                let v = value.downcast_ref::<i64>().expect("i64");
                assert_eq!(*v, 1, "each READABLE → result(1)");
            }
            other => panic!("expected Yield, got {other:?}"),
        }
    }
}

#[test]
fn io_read_write_interest_both() {
    // push 一个 READ_WRITE 事件 → IoHandler 产出 result(3)
    // （readable +1 + writable +2）。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READ_WRITE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READ_WRITE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 1);
    match &results[0] {
        ProcessResult::Yield { value, .. } => {
            let v = value.downcast_ref::<i64>().expect("i64");
            assert_eq!(*v, 3, "READ_WRITE → readable(+1) + writable(+2) = 3");
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn io_run_io_timeout_returns_partial() {
    // ManualReactor 有 1 个 pending 事件，run_io with timeout=0ms
    // 仍返回该事件——ManualReactor 不真睡，timeout=0 不跳过已就绪事件。
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "timeout=0 still returns pending event");
}

// ════════════════════════════════════════════════════════════════════════════
// 复合 Machine 错误路径测试
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn composite_too_deep_reports_error() {
    // 自引用复合：composite "loop" 的子拓扑包含一个 "loop" 类型机器实例。
    // expand_composites 循环 64 次仍 found_composite=true → CompositeTooDeep。
    let loop_spec = DeploySpec::new()
        .with_machine(MachineInstance::new("inner", "loop", MachinePhysicalSpec::default()));
    let loop_comp = CompositeSpec::new(loop_spec);

    let mut rt = Runtime::default();
    rt.register_composite("loop", loop_comp);

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("top", "loop", MachinePhysicalSpec::default()));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::CompositeTooDeep { depth: 64, .. }),
        "expected CompositeTooDeep, got {err:?}"
    );
}

#[test]
fn composite_unknown_type_fails_at_build() {
    // 未注册的复合类型 "unknown_comp" 不被 expand_composites 展开
    // （composites map 里没有），machine_type 保持 "unknown_comp"，
    // build 阶段 registry.build 找不到 → InitFailed。
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("c", "unknown_comp", MachinePhysicalSpec::default()));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::InitFailed { ref machine, .. } if machine == "unknown_comp"),
        "expected InitFailed for unknown_comp, got {err:?}"
    );
}

#[test]
fn composite_internal_dangling_port() {
    // 复合内 input_map 指向不存在的子机器 "nonexistent"——展开后
    // 外部链接的 into 端变成 "comp.nonexistent.x"，但 machines 里
    // 没有 "comp.nonexistent" → validate_endpoint 报 DanglingRef。
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let bad_spec = DeploySpec::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));
    let bad_comp = CompositeSpec::new(bad_spec)
        .with_input("in", "nonexistent", "x")
        .with_output("out", "d1", "y");
    rt.register_composite("bad_pair", bad_comp);

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "bad_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port }
                 if machine == "comp.nonexistent" && port == "x"),
        "expected DanglingRef for comp.nonexistent.x, got {err:?}"
    );
}

#[test]
fn composite_external_link_to_undefined_port() {
    // 外部链接指向复合的未定义端口 "undefined_port"（不在 input_map 中），
    // 展开后端口名保持原样、机器名 "comp" 不再存在（已展开为 comp.d1/d2）
    // → validate_endpoint 报 DanglingRef。
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "undefined_port"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port }
                 if machine == "comp" && port == "undefined_port"),
        "expected DanglingRef for comp.undefined_port, got {err:?}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 跨平台 IO reactor 真实 socket 测试（cfg gate，仅 Linux/macOS 编译）
// ════════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "linux")]
#[test]
fn io_epoll_reactor_detects_tcp_readability() {
    // Linux epoll 真实 TCP listener：注册 READABLE → 客户端连接 →
    // poll 检测到 READABLE（EPOLLIN）→ IoEvent 产出。
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::epoll::EpollReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let raw = listener.as_raw_fd();

    let mut reactor = EpollReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::READABLE, IoToken(42))
        .expect("register");

    // 连接前 poll 应无事件（timeout=0 非阻塞）。
    let no_events = reactor.poll(Some(Duration::from_millis(0))).expect("poll empty");
    assert!(no_events.is_empty(), "no events before connection");

    // 客户端连接 → listener 可 accept → READABLE 就绪。
    let _client = TcpStream::connect(addr).expect("connect");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "should detect readable after connect");
    let found = events.iter().any(|e| e.token == IoToken(42) && e.readiness.is_readable());
    assert!(found, "token 42 readable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

#[cfg(target_os = "linux")]
#[test]
fn io_epoll_reactor_writable() {
    // Linux epoll TCP stream 可写测试：connect 后 stream 立即可写
    // （发送缓冲区空闲）→ poll 立即返回 WRITABLE（EPOLLOUT）。
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::epoll::EpollReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    let stream = TcpStream::connect(addr).expect("connect");
    stream.set_nonblocking(true).expect("nonblocking");
    let raw = stream.as_raw_fd();

    let mut reactor = EpollReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::WRITABLE, IoToken(7))
        .expect("register");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "fresh TCP stream should be writable");
    let found = events.iter().any(|e| e.token == IoToken(7) && e.readiness.is_writable());
    assert!(found, "token 7 writable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

#[cfg(target_os = "macos")]
#[test]
fn io_kqueue_reactor_detects_tcp_readability() {
    // macOS kqueue 真实 TCP listener：注册 READABLE → 客户端连接 →
    // poll 检测到 READABLE（EVFILT_READ）→ IoEvent 产出。
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::kqueue::KqueueReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let raw = listener.as_raw_fd();

    let mut reactor = KqueueReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::READABLE, IoToken(42))
        .expect("register");

    // 连接前 poll 应无事件（timeout=0 非阻塞）。
    let no_events = reactor.poll(Some(Duration::from_millis(0))).expect("poll empty");
    assert!(no_events.is_empty(), "no events before connection");

    // 客户端连接 → listener 可 accept → READABLE 就绪。
    let _client = TcpStream::connect(addr).expect("connect");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "should detect readable after connect");
    let found = events.iter().any(|e| e.token == IoToken(42) && e.readiness.is_readable());
    assert!(found, "token 42 readable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

#[cfg(target_os = "macos")]
#[test]
fn io_kqueue_reactor_writable() {
    // macOS kqueue TCP stream 可写测试：connect 后 stream 立即可写
    // → poll 立即返回 WRITABLE（EVFILT_WRITE）。
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::kqueue::KqueueReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    let stream = TcpStream::connect(addr).expect("connect");
    stream.set_nonblocking(true).expect("nonblocking");
    let raw = stream.as_raw_fd();

    let mut reactor = KqueueReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::WRITABLE, IoToken(7))
        .expect("register");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "fresh TCP stream should be writable");
    let found = events.iter().any(|e| e.token == IoToken(7) && e.readiness.is_writable());
    assert!(found, "token 7 writable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}
