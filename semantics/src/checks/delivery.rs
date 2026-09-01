//! 投递四态税则（义务类 D1 的投递态分量；meta-foundations 定义 1.6、命题 7.4）。
//!
//! 四态：Full（满，值未投递，可重试或阻塞）、Closed（断连，值随错误回传、
//! 不消失）、Timeout（等待超时）、Cancelled（请求方已放弃）。
//!
//! **落位律（A3）**：Full/Closed 在本模块机械化（模态②③：从标准库错误直接映射、
//! 装配点可校验）；Timeout/Cancelled 需要定时器与请求域通道机制——按 A5 诚实规则
//! 声明为模态④，不伪造见证（不提供伪判定的 API）。
//!
//! **Timeout 升级注记（2026-09-01 权威变更，§0.6 终局判据第三要素兑现）**：
//! 期限等待点分量的 Timeout 见证已于异步域机械化（②③）——真定时器直接映射
//! （②面：异步接缝 `TimedOut` 由运行时定时器驱动，非伪造判定）＋行为可验证
//! （③面：T6 对拍同期限同裁决、期限下限断言，`instances/tests/t6_crosscheck.rs`）。
//! 本模块的 ④ 声明域据此收窄：`Delivery` 级 `Timeout`/`Cancelled` 构造器仍缺位
//! （投递域超时与请求域取消未落地），无构造器边界测试不变。
//!
//! **Timeout/Cancelled 的让渡合同（N1 捆绑条款：④ 声明域 = 有对价的合同）**：
//! - ①让渡了什么：投递域超时/取消的见证——调用方无法凭契约区分“慢”与“死”
//!   （期限等待点分量已由上述升级收回，不再让渡）；
//! - ②为什么让渡：投递域定时器与请求域通道是物理层机制，编译期不可判定
//!   （H2 封顶）；违反后果是“变慢/可重试”（可逆），不满足入定义面（①②）的判据；
//! - ③代价由谁承担：调用方/上层——自建期限监视，或经异步接缝的 `TimedOut`
//!   机制地面（已完成：真定时器驱动，升 ②③ 权威变更已执行）；
//! - 边界测试：本模块无 Timeout/Cancelled 构造器、无伪判定 API——声明域不得
//!   无声扩张（不得悄悄变成“有超时”的承诺，也不得把可机械化态移入④）。
//!
//! 值守恒：Full/Closed 携带被拒值（对齐 buffer.rs"不静默丢值"纪律）。

/// 投递结果（发送侧）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery<T> {
    /// 已投递。
    Delivered,
    /// 满：通道容量已尽，值未投递（可重试或阻塞）。模态②/③ 可见证。
    Full(T),
    /// 断连：对端已关闭，值未投递，随错误回传（不消失）。模态②/③ 可见证。
    Closed(T),
}

impl<T> Delivery<T> {
    /// 从 `mpsc::SendError` 映射：仅断连可发生（值随错误回传）。
    pub fn from_send_error(err: std::sync::mpsc::SendError<T>) -> Self {
        Delivery::Closed(err.0)
    }

    /// 从 `mpsc::TrySendError` 映射：Full 与 Disconnected 显式区分，值均保留。
    pub fn from_try_send_error(err: std::sync::mpsc::TrySendError<T>) -> Self {
        match err {
            std::sync::mpsc::TrySendError::Full(v) => Delivery::Full(v),
            std::sync::mpsc::TrySendError::Disconnected(v) => Delivery::Closed(v),
        }
    }

    /// 是否已投递。
    pub fn is_delivered(&self) -> bool {
        matches!(self, Delivery::Delivered)
    }

    /// 取回被拒值（Full/Closed 均携带）。
    pub fn into_rejected(self) -> Option<T> {
        match self {
            Delivery::Delivered => None,
            Delivery::Full(v) | Delivery::Closed(v) => Some(v),
        }
    }
}

/// 接收侧结果（空与断连显式区分；对齐 `BoundedQueue::pop/try_pop` 语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt<T> {
    /// 收到值。
    Item(T),
    /// 此刻为空（可重试）。模态②/③ 可见证。
    Empty,
    /// 发送端断连。模态②/③ 可见证。
    Closed,
}

impl<T> Receipt<T> {
    /// 从 `mpsc::TryRecvError` 映射。
    pub fn from_try_recv_error(err: std::sync::mpsc::TryRecvError) -> Self {
        match err {
            std::sync::mpsc::TryRecvError::Empty => Receipt::Empty,
            std::sync::mpsc::TryRecvError::Disconnected => Receipt::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_send_maps_full_and_closed_distinctly() {
        use std::sync::mpsc::sync_channel;
        let (tx, rx) = sync_channel::<i32>(1);
        let _ = rx; // 保留接收端防断连
        // 空出 1 个槽位后：第二投即 Full。
        tx.send(1).ok();
        let full = tx.try_send(2).unwrap_err();
        assert_eq!(
            Delivery::from_try_send_error(full),
            Delivery::Full(2),
            "Full 必须携带被拒值"
        );
        drop(rx);
        let closed = tx.try_send(3).unwrap_err();
        assert_eq!(
            Delivery::from_try_send_error(closed),
            Delivery::Closed(3),
            "断连必须与满区分且值回传"
        );
    }

    #[test]
    fn send_error_is_closed_with_value() {
        // mpsc::SendError 仅在断连时出现，值随错误回传——映射须保真。
        let err = std::sync::mpsc::SendError(7);
        assert_eq!(Delivery::from_send_error(err), Delivery::Closed(7));
    }

    #[test]
    fn receipts_distinguish_empty_and_closed() {
        // 通道行为由 buffer.rs 测试覆盖；此处验证映射函数的显式区分。
        use std::sync::mpsc::TryRecvError;
        assert_eq!(
            Receipt::from_try_recv_error(TryRecvError::Empty),
            Receipt::Empty::<()>
        );
        assert_eq!(
            Receipt::from_try_recv_error(TryRecvError::Disconnected),
            Receipt::Closed::<()>
        );
    }

    /// P6（投递格复合性，closure-audit 3.5）最小复合反例。
    ///
    /// 命题：可分解验证（T4）依赖投递格对复合封闭——复合体边界的判定
    /// 可由分段判定复合得出。本测试装置化一条"有损边"（饱和即无声丢弃，
    /// 模拟外部分析中存在的 Drop 型边；非词汇表成员），证明：
    /// - 无损边（Fail 策略，`Full(v)` 值随判回传）下复合成立：外层判定
    ///   与分段判定逐条对应（值守恒，复合可分解）；
    /// - 有损边（Drop 策略）下复合破坏：外层判定为 `Delivered`，而分段
    ///   判定的真实值（Drop）不在四态格内——`Delivered` 无法由格内分段
    ///   判定复合得出。
    ///
    /// 结论（P6 可证伪预测的机械化见证）：凡载体边以投递四态格声明判定
    /// （无 Drop），复合验证可分解；引入有损边则复合性破坏，除非饱和
    /// 策略升格为显式判定。预测绑定下一个被研究系统验证（§0.13 预测先行
    /// 协议）；命中则 P6 升格为投递格公理条款。
    #[test]
    fn p6_drop_edge_breaks_verdict_compositionality() {
        use alloc::collections::VecDeque;

        /// 测试装置：有界边。`lossy = false` 为 Fail 策略（满则 Full 回传，
        /// 判定守恒）；`lossy = true` 为 Drop 策略（满则无声丢弃，外层仍
        /// 报 Delivered）。装置仅存在于本测试，不进词汇表。
        struct Edge<T> {
            q: VecDeque<T>,
            cap: usize,
            lossy: bool,
            dropped: usize,
        }
        impl<T> Edge<T> {
            fn push(&mut self, v: T) -> Delivery<()> {
                if self.q.len() < self.cap {
                    self.q.push_back(v);
                    Delivery::Delivered
                } else if self.lossy {
                    self.dropped += 1;
                    Delivery::Delivered // 有损边：外层判定照报成功（反例核心）
                } else {
                    Delivery::Full(()) // Fail 策略：满即判定，值守恒由调用方持 v 重试
                }
            }
        }

        // 面 1：无损边复合可分解——3 投 1 容量：Delivered / Full / Full，
        // 分段判定与外层判定一一对应（值不消失）。
        let mut solid = Edge { q: VecDeque::new(), cap: 1, lossy: false, dropped: 0 };
        let mut held = Vec::new();
        let outer: Vec<_> = [1, 2, 3]
            .into_iter()
            .map(|v| match solid.push(v) {
                Delivery::Delivered => Delivery::<i32>::Delivered,
                Delivery::Full(()) => {
                    held.push(v);
                    Delivery::Full(v)
                }
                Delivery::Closed(()) => unreachable!("装置无边断连"),
            })
            .collect();
        assert_eq!(
            outer,
            vec![Delivery::Delivered, Delivery::Full(2), Delivery::Full(3)]
        );
        assert_eq!(solid.q.len() + held.len(), 3, "Fail 边：值全部存活（守恒，复合可分解）");

        // 面 2：有损边复合破坏——同 3 投 1 容量：外层全 Delivered，
        // 但仅 1 值入格，2 值无声消失；分段真实判定（Drop）不在四态格内，
        // 外层 Delivered 无法由格内分段判定复合得出。
        let mut lossy = Edge { q: VecDeque::new(), cap: 1, lossy: true, dropped: 0 };
        let outer: Vec<_> = [1, 2, 3].into_iter().map(|v| lossy.push(v)).collect();
        assert_eq!(
            outer,
            vec![Delivery::Delivered, Delivery::Delivered, Delivery::Delivered],
            "有损边：外层判定全报成功"
        );
        assert_eq!(lossy.q.len(), 1, "仅 1 值真正入格");
        assert_eq!(lossy.dropped, 2, "2 值无声消失——四态格在复合处漏值（P6 反例成立）");
    }
}