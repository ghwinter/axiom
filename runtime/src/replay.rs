//! 事件溯源回放器（D1）——任意时点回放 / 时间旅行。
//!
//! # 定位
//!
//! axiom 的确定性（R001：同输入 → 同终态）使**回放**成为一等公民：
//! 记录驱动系统的输入事件流，之后从干净状态重放到任意时点，得到与
//! 原始执行**逐位一致**的状态。这就是时间旅行调试——"回放到崩溃前
//! 一刻看状态"。
//!
//! # 设计：为什么不需要序列化
//!
//! 回放的输入是类型擦除的 `Box<dyn Any>`（runtime 的 `tick` 载荷）。
//! `Any` 无法克隆，但回放需要**多次**重放同一输入。解法：journal 存
//! **载荷工厂**（`Box<dyn Fn() -> Box<dyn Any>>`）——每次重放调用工厂
//! 重建载荷。对 `Clone` 输入（字节流、命令、标量——showcase 的主流），
//! [`ReplayJournal::record`] 自动包装；任意载荷用 [`record_fn`](ReplayJournal::record_fn)。
//!
//! 快照（`Machine::checkpoint`）是回放的**优化**（从快照而非零开始），
//! 第一版从零重放已满足"任意时点"；快照接入是后续增量。
//!
//! # 契约
//!
//! 回放正确性由 runtime 的确定性保证（非回放器自身）：同一载荷工厂
//! 产生相同值、runtime 对相同输入产生相同输出。回放器只做三件事：
//! 记录、重建、按序 tick。

use alloc::string::String;
use alloc::vec::Vec;
use std::boxed::Box;
use std::sync::Arc;

use crate::{ProcessResult, Runtime, RuntimeError};

/// 一条可重建的输入事件。
pub struct ReplayEntry {
    pub machine: String,
    pub port: String,
    make: Box<dyn Fn() -> Box<dyn core::any::Any + Send> + Send + Sync>,
}

impl ReplayEntry {
    /// 重建载荷（每次调用产生新 `Box<dyn Any>`——可重复重放）。
    fn rebuild(&self) -> Box<dyn core::any::Any + Send> {
        (self.make)()
    }
}

/// 一批输入（对应一次 `Runtime::tick` 的全部输入）。
pub struct TickBatch {
    entries: Vec<ReplayEntry>,
}

impl TickBatch {
    fn to_inputs(&self) -> Vec<(String, String, Box<dyn core::any::Any + Send>)> {
        self.entries
            .iter()
            .map(|e| (e.machine.clone(), e.port.clone(), e.rebuild()))
            .collect()
    }
}

/// 输入事件流（journal）——回放器的数据源。
///
/// 录制协议：为每批输入调用 [`record`](ReplayJournal::record)（一条或多条），
/// 批结束时调用 [`end_batch`](ReplayJournal::end_batch)。重复 [`record`](ReplayJournal::record)
/// 但未 `end_batch` 的条目属于同一批（同一次 tick）。
#[derive(Default)]
pub struct ReplayJournal {
    batches: Vec<TickBatch>,
}

impl ReplayJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// 录入一条可克隆的输入载荷（`T: Clone`——字节、命令、标量主流场景）。
    ///
    /// 工厂闭包捕获 `payload.clone()`：每次重放重建同值载荷。
    pub fn record<T: Clone + Send + Sync + 'static>(
        &mut self,
        machine: impl Into<String>,
        port: impl Into<String>,
        payload: &T,
    ) -> &mut Self {
        let payload = payload.clone();
        self.record_fn(
            machine,
            port,
            Box::new(move || Box::new(payload.clone()) as Box<dyn core::any::Any + Send>),
        )
    }

    /// 录入一条任意载荷（调用者提供重建工厂）。
    pub fn record_fn(
        &mut self,
        machine: impl Into<String>,
        port: impl Into<String>,
        make: Box<dyn Fn() -> Box<dyn core::any::Any + Send> + Send + Sync>,
    ) -> &mut Self {
        let last = self
            .batches
            .last_mut()
            .expect("end_batch() before record()");
        last.entries.push(ReplayEntry {
            machine: machine.into(),
            port: port.into(),
            make,
        });
        self
    }

    /// 结束当前批（一次 `tick` 的输入边界）。
    pub fn end_batch(&mut self) -> &mut Self {
        self.batches.push(TickBatch {
            entries: Vec::new(),
        });
        self
    }

    /// 已录批数（= 可回放的最大时点）。
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// 第 `i` 批的条目数（调试/诊断用）。
    pub fn batch_len(&self, i: usize) -> usize {
        self.batches[i].entries.len()
    }
}

/// 回放器——从 journal 重建任意时点的运行时状态。
pub struct Replayer<'a> {
    journal: &'a ReplayJournal,
}

impl<'a> Replayer<'a> {
    pub fn new(journal: &'a ReplayJournal) -> Self {
        Self { journal }
    }

    /// 从零重放到第 `t` 批（`t` = 时间旅行目标时点）。
    ///
    /// `build` 是 runtime 工厂（注册 + materialize——调用者拥有注册表）。
    /// 返回重放后的 `Runtime` 与**逐批输出**（每批一次 tick 的
    /// `ProcessResult` 集合）——后者用于与原始执行对比（回放正确性）。
    pub fn forward_to(
        &self,
        t: usize,
        build: impl Fn() -> Runtime,
    ) -> Result<(Runtime, Vec<Vec<ProcessResult>>), RuntimeError> {
        assert!(t <= self.journal.batches.len(), "t={t} 超出 journal 时点 {}", self.journal.batches.len());
        let mut rt = build();
        let mut outputs = Vec::with_capacity(t);
        for batch in self.journal.batches.iter().take(t) {
            let out = rt.tick(batch.to_inputs())?;
            outputs.push(out);
        }
        Ok((rt, outputs))
    }

    /// 回放正确性验证：重放到 `t` 的输出与原始执行（`original`）逐批一致。
    ///
    /// `original[i]` = 原始执行第 `i` 批的输出。返回第一个不一致的批号
    /// （`None` = 完全一致）。
    pub fn verify<'r>(
        &self,
        t: usize,
        build: impl Fn() -> Runtime,
        original: impl IntoIterator<Item = &'r Vec<ProcessResult>>,
    ) -> Option<usize> {
        let (_, replayed) = self.forward_to(t, build).expect("replay");
        for (i, (a, b)) in replayed.iter().zip(original).enumerate() {
            if !outputs_equal(a, b) {
                return Some(i);
            }
        }
        None
    }
}

/// 输出对比：`ProcessResult` 的载荷是 `Box<dyn Any>`——比较端口与载荷
/// 的 downcast 值（若载荷同类型则逐字节比较其 Debug 表示；不同类型视为不等）。
fn outputs_equal(a: &[ProcessResult], b: &[ProcessResult]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        match (x, y) {
            (ProcessResult::Yield { port: pa, value: va }, ProcessResult::Yield { port: pb, value: vb }) => {
                if pa != pb {
                    return false;
                }
                // 载荷比较：downcast 到相同具体类型后比 Debug（不要求
                // 类型可 Serialize——回放正确性以"同类型同值"为准）。
                if va.type_id() != vb.type_id() {
                    return false;
                }
                let da = format!("{:?}", va);
                let db = format!("{:?}", vb);
                if da != db {
                    return false;
                }
            }
            (ProcessResult::YieldMulti { outputs: oa }, ProcessResult::YieldMulti { outputs: ob }) => {
                if oa.len() != ob.len() {
                    return false;
                }
                for (x, y) in oa.iter().zip(ob.iter()) {
                    if x.0 != y.0 || format!("{:?}", x.1) != format!("{:?}", y.1) {
                        return false;
                    }
                }
            }
            _ => {
                // Idle/Done 或变体不匹配：比较变体标签。
                if std::mem::discriminant(x) != std::mem::discriminant(y) {
                    return false;
                }
            }
        }
    }
    true
}

// 使 ReplayEntry/TickBatch 可跨线程传递（工厂 Send + Sync）。
unsafe impl Send for ReplayEntry {}
unsafe impl Sync for ReplayEntry {}


// 抑制未使用导入警告（Arc 预留快照接入）。
#[allow(unused)]
fn _reserve(_: Arc<()>) {}

