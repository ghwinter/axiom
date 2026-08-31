//! # TracingTelemetry — 语义层 `Telemetry` 契约的 tracing 实现（第二实现者）
//!
//! 语义层 [`Telemetry`](axiom_semantics::seams::telemetry::Telemetry) 插座的
//! **tracing 绑定**：把接缝裁决/深度/延迟事件转发为 tracing 事件（结构化字段，
//! 级别契约见下）。树内第一实现是语义层自带的 `ConsoleTelemetry`/`BufTelemetry`
//! （println/缓冲）；本模块是 Telemetry 插座的**第二实现者**——按极小基律，插座的
//! 扩展由真实观测需求引入，本模块即首个此类绑定。
//!
//! ## 级别契约（声明的映射，非约定俗成）
//!
//! | 事件 | tracing 级别 | 理由 |
//! |---|---|---|
//! | `on_verdict(Delivered)` | `trace` | 热路径：每次成功投递都发生，默认订阅者（info）不可见 |
//! | `on_verdict(Full/Failed/Dropped)` | `warn` | 例外：饱和/失败/丢失值得被默认看见 |
//! | `on_depth` | `debug` | 背压观测：按需打开（`RUST_LOG=debug`） |
//! | `on_latency` | `trace` | 采样可能高频：仅诊断期打开 |
//!
//! 输出目的地（控制台/文件/JSON/OTLP）由**订阅者**决定，不由本适配器决定——
//! 这是"打印 vs 观测"边界的机制化：**代码只发事件，格式与目的地是部署决策**。
//!
//! 目标（target）：`axiom_instances::telemetry_tracing`，可用 `RUST_LOG=axiom_instances=debug` 单独过滤。
//!
//! 门控：`telemetry-tracing` feature（拉语义层 `telemetry` + 可选依赖 `tracing`）。

use axiom_semantics::seams::telemetry::{Telemetry, VerdictView};

/// re-export：观测面契约（语义层定义；消费端只需依赖实例层 + 本 feature，
/// 无需直依赖语义层即可命名 `Telemetry`/`VerdictView`——与 `async_ring` 的
/// 契约 re-export 同纪律）。
pub use axiom_semantics::seams::telemetry::{Telemetry as TelemetrySink, VerdictView as SeamVerdict};

/// tracing 观测汇：零大小（状态在订阅者侧，本结构无状态）。
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingTelemetry;

impl TracingTelemetry {
    /// 新建（零大小，`Default` 等价）。
    pub fn new() -> Self {
        Self
    }
}

impl Telemetry for TracingTelemetry {
    fn on_verdict(&mut self, seam: &'static str, v: VerdictView) {
        match v {
            VerdictView::Delivered => {
                tracing::trace!(target: "axiom_instances::telemetry_tracing", seam = %seam, verdict = ?v, "seam verdict");
            }
            VerdictView::Full | VerdictView::Failed | VerdictView::Dropped => {
                tracing::warn!(target: "axiom_instances::telemetry_tracing", seam = %seam, verdict = ?v, "seam verdict (exceptional)");
            }
        }
    }

    fn on_depth(&mut self, seam: &'static str, depth: usize) {
        tracing::debug!(target: "axiom_instances::telemetry_tracing", seam = %seam, depth = depth, "seam depth");
    }

    fn on_latency(&mut self, seam: &'static str, nanos: u64) {
        tracing::trace!(target: "axiom_instances::telemetry_tracing", seam = %seam, latency_ns = nanos, "seam latency");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Record};
    use tracing::{Event, Id, Metadata, Subscriber};

    /// 捕获订阅者：记录 (target, level, fields) 三元组（测试专用，零依赖）。
    #[derive(Default)]
    struct Capture(Mutex<Vec<String>>);

    struct FieldVisitor(Vec<String>);
    impl Visit for FieldVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push(format!("{}={}", field.name(), value));
        }
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.push(format!("{}={:?}", field.name(), value));
        }
        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.push(format!("{}={}", field.name(), value));
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, _meta: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _attrs: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _span: &Id, _values: &Record<'_>) {}
        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut vis = FieldVisitor(Vec::new());
            event.record(&mut vis);
            let line = format!(
                "{}|{}|{}",
                event.metadata().target(),
                event.metadata().level(),
                vis.0.join(",")
            );
            self.0.lock().unwrap().push(line);
        }
        fn enter(&self, _span: &Id) {}
        fn exit(&self, _span: &Id) {}
    }

    #[test]
    fn verdicts_map_to_declared_levels_and_fields() {
        let cap = Arc::new(Capture::default());
        let c2 = cap.clone();
        tracing::subscriber::with_default(c2, || {
            let mut tel = TracingTelemetry::new();
            tel.on_verdict("edge-a", VerdictView::Delivered);
            tel.on_verdict("edge-a", VerdictView::Full);
            tel.on_verdict("edge-a", VerdictView::Dropped);
            tel.on_depth("edge-a", 3);
            tel.on_latency("edge-a", 1_234);
        });
        let lines = cap.0.lock().unwrap().clone();
        assert_eq!(lines.len(), 5, "5 个事件全部到达订阅者");
        // 级别契约：Delivered=TRACE；Full/Dropped=WARN。
        let (l0, l1, l2, l3, l4) = (&lines[0], &lines[1], &lines[2], &lines[3], &lines[4]);
        assert!(l0.contains("TRACE") && l0.contains("seam=edge-a"), "{l0}");
        assert!(l1.contains("WARN") && l1.contains("verdict=Full"), "{l1}");
        assert!(l2.contains("WARN") && l2.contains("verdict=Dropped"), "{l2}");
        // 深度=DEBUG、结构化字段 depth=3。
        assert!(l3.contains("DEBUG") && l3.contains("depth=3"), "{l3}");
        // 延迟=TRACE、字段 latency_ns=1234。
        assert!(l4.contains("TRACE") && l4.contains("latency_ns=1234"), "{l4}");
        // 全部命中适配器 target（可被 RUST_LOG=axiom_instances 单独过滤）。
        assert!(lines.iter().all(|l| l.starts_with("axiom_instances::telemetry_tracing|")));
    }

    #[test]
    fn adapter_is_zero_sized_state() {
        // 无状态：观测数据全部在订阅者侧（打印 vs 观测边界的机制化——
        // 适配器只转发，不持有任何事件）。
        assert_eq!(std::mem::size_of::<TracingTelemetry>(), 0);
    }
}
