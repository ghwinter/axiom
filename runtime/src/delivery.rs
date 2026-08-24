//! 投递四态税则（义务类 D1 的投递态分量；meta-foundations 定义 1.6、命题 7.4）。
//!
//! 四态：**Full**（满，值未投递，可重试或阻塞）、**Closed**（断连，值随错误回传、
//! 不消失）、**Timeout**（等待超时）、**Cancelled**（请求方已放弃）。
//!
//! **落位律（A3）**：Full/Closed 在本模块**机械化**（模态②③：从标准库错误直接映射、
//! 装配点可校验）；Timeout/Cancelled 需要定时器与请求域通道机制——按 A5 诚实规则
//! 声明为模态④，**不伪造见证**（不提供假装判定的 API）。机械化为物理选择，待真异步
//! 载体时落地（runtime-constitution 阶段 4）。
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
}