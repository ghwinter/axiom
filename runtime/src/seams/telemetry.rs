//! 观测面标准接口（B1）——每接缝的投递/深度/延迟遥测；默认 no-op。
//!
//! 观测面语义（runtime-constitution §8）：被观测信息的**收集 → 输出目的地**。
//! 本模块给标准接口（[`Telemetry`]）与两种现成实现（缓冲/控制台）；输出目的地
//! （日志/持久化）由实现者按其载体选择。与义务账本的分工：账本管**承诺**
//! （模态 ①–④）；遥测观测**兑现**（投递/失败/深度/延迟的实际事件）。
//!
//! 成本：默认方法体为空 → 内联后编译期零成本（no-op 不付税，热路径安全）。

use alloc::vec::Vec;

/// 接缝裁决的观测视图（与投递语义一致：值不静默消失）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictView {
    /// 已投递（消费侧可收）。
    Delivered,
    /// 饱和：值随判定回传（`Full(v)` 语义）。
    Full,
    /// 变换失败（短路，失败为值）。
    Failed,
    /// 拆除/断连：未投递（`dropped`/`Closed` 语义）。
    Dropped,
}

/// 遥测接收面：接缝观测事件。默认方法为空（no-op，编译期零成本）。
pub trait Telemetry {
    /// 一条投递裁决事件。
    fn on_verdict(&mut self, _seam: &'static str, _v: VerdictView) {}
    /// 队列深度采样（背压观测）。
    fn on_depth(&mut self, _seam: &'static str, _depth: usize) {}
    /// 延迟采样（纳秒；直方图聚合属实现者）。
    fn on_latency(&mut self, _seam: &'static str, _nanos: u64) {}
}

impl<T: Telemetry + ?Sized> Telemetry for &mut T {
    fn on_verdict(&mut self, seam: &'static str, v: VerdictView) {
        (**self).on_verdict(seam, v);
    }
    fn on_depth(&mut self, seam: &'static str, depth: usize) {
        (**self).on_depth(seam, depth);
    }
    fn on_latency(&mut self, seam: &'static str, nanos: u64) {
        (**self).on_latency(seam, nanos);
    }
}

/// 默认 no-op：全部方法为空，内联后零成本。
pub struct NoOpTelemetry;
impl Telemetry for NoOpTelemetry {}

/// 缓冲遥测（测试/示例用）：按序收集事件。
#[derive(Debug, Default, Clone)]
pub struct BufTelemetry {
    /// 裁决事件序列（接缝, 视图）。
    pub verdicts: Vec<(&'static str, VerdictView)>,
    /// 深度采样序列（接缝, 深度）。
    pub depths: Vec<(&'static str, usize)>,
    /// 延迟采样序列（接缝, 纳秒）。
    pub latencies: Vec<(&'static str, u64)>,
}

impl BufTelemetry {
    /// 新建空缓冲。
    pub fn new() -> Self {
        Self::default()
    }
}

impl Telemetry for BufTelemetry {
    fn on_verdict(&mut self, seam: &'static str, v: VerdictView) {
        self.verdicts.push((seam, v));
    }
    fn on_depth(&mut self, seam: &'static str, depth: usize) {
        self.depths.push((seam, depth));
    }
    fn on_latency(&mut self, seam: &'static str, nanos: u64) {
        self.latencies.push((seam, nanos));
    }
}

/// 控制台遥测（输出目的地示例之一）：直接打印。std 门控。
#[cfg(feature = "std")]
pub struct ConsoleTelemetry;

#[cfg(feature = "std")]
impl Telemetry for ConsoleTelemetry {
    fn on_verdict(&mut self, seam: &'static str, v: VerdictView) {
        println!("[telemetry] {seam}: verdict {v:?}");
    }
    fn on_depth(&mut self, seam: &'static str, depth: usize) {
        println!("[telemetry] {seam}: depth {depth}");
    }
    fn on_latency(&mut self, seam: &'static str, nanos: u64) {
        println!("[telemetry] {seam}: latency {nanos} ns");
    }
}

/// 投递闭包的遥测包装：把 `f` 的每次裁决转发给 `tel`（接线点：`pump_events`
/// 的 `push` 内——由调用方把 `event::PushVerdict` 映射为 [`VerdictView`]；
/// `SeamPoller::roll` 的裁决处同理）。
///
/// `f` 返回 `VerdictView`（或可映射的值）；包装先上报再原样返回。
/// 本模块仅依赖 core/alloc（no_std 可用）。
pub struct MeteredPush<F, T> {
    seam: &'static str,
    f: F,
    tel: T,
}

impl<F, T> MeteredPush<F, T> {
    /// 新建：把 `seam` 的裁决经 `tel` 上报。
    pub fn new(seam: &'static str, f: F, tel: T) -> Self {
        MeteredPush { seam, f, tel }
    }
}

impl<F, T> MeteredPush<F, T>
where
    F: FnMut(VerdictView) -> VerdictView,
    T: Telemetry,
{
    /// 调用包装：上报并转发。
    pub fn call(&mut self, v: VerdictView) -> VerdictView {
        self.tel.on_verdict(self.seam, v);
        (self.f)(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buf_telemetry_collects_in_order() {
        let mut tel = BufTelemetry::new();
        tel.on_verdict("seam-a", VerdictView::Delivered);
        tel.on_verdict("seam-a", VerdictView::Full);
        tel.on_depth("seam-a", 2);
        tel.on_latency("seam-a", 42);
        assert_eq!(
            tel.verdicts,
            vec![
                ("seam-a", VerdictView::Delivered),
                ("seam-a", VerdictView::Full)
            ]
        );
        assert_eq!(tel.depths, vec![("seam-a", 2)]);
        assert_eq!(tel.latencies, vec![("seam-a", 42)]);
    }

    #[test]
    fn no_op_is_silent_and_zero_sized_effect() {
        // no-op 语义：调用无副作用（零成本面）。
        let mut tel = NoOpTelemetry;
        tel.on_verdict("anything", VerdictView::Failed);
        tel.on_depth("anything", usize::MAX);
        tel.on_latency("anything", u64::MAX);
    }

    #[test]
    fn metered_push_forwards_and_reports() {
        // 接线点演示：包装判定 → 遥测记录 → 原样转发（调用方自行映射
        // event::PushVerdict → VerdictView）。
        let mut tel = BufTelemetry::new();
        let mut m = MeteredPush::new(
            "pump-edge",
            |v| v, // 透传
            &mut tel,
        );
        assert_eq!(m.call(VerdictView::Delivered), VerdictView::Delivered);
        assert_eq!(
            tel.verdicts,
            vec![("pump-edge", VerdictView::Delivered)]
        );
    }
}