//! 受控共享数据——封装与组合的折中。
//!
//! `Machine` 默认**封装**状态（局部性、可验证：状态只被自己的 `process`
//! 改），但跨机器数据共享受限。本模块提供 [`SharedResource`]——一个可被
//! 多个计算单元声明读写的全局单例数据——兼得封装的局部性与数据驱动的
//! 组合性（共享数据原语（`Resource` 类）在 axiom 的受控形态）。
//!
//! # 与 `Machine` 封装的关系
//!
//! - 默认：机器状态有主（`Machine::State` 只被自己的 `process` 修改）。
//! - 需要跨机器共享的数据：用 `SharedResource` **显式**承载——共享是声明
//!   式的（构造一个句柄，传给使用方），而非隐式全局。
//! - 读写经 `RwLock`：多个读者可并行、写者互斥（对应调度可验证性 D8——
//!   多写者需显式串行，`SharedResource` 的 `write()` 正是互斥点）。
//!
//! # 零成本说明
//!
//! `SharedResource` 是**物理层原语**（锁保护共享内存），不替代抽象层的
//! 端口/拓扑验证；它只在"确实需要跨机器共享"时引入锁开销——不需要共享
//! 的机器保持零成本封装。

#[cfg(feature = "std")]
use alloc::sync::Arc;
#[cfg(feature = "std")]
use std::sync::RwLock;

/// 受控共享数据：多个计算单元共享的全局单例。
///
/// - [`read`](Self::read)：共享读（多个读者可并行）；
/// - [`write`](Self::write)：独占写（写者互斥）；
/// - [`clone_handle`](Self::clone_handle)：复制共享句柄（底层同一份数据）。
///
/// 仅 `std` 提供（`RwLock`）；`no_std` 配置不含本原语。
#[cfg(feature = "std")]
pub struct SharedResource<T> {
    inner: Arc<RwLock<T>>,
}

#[cfg(feature = "std")]
impl<T> SharedResource<T> {
    /// 构造共享资源（初始值）。
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(value)),
        }
    }

    /// 共享读——返回读锁（多个读者可并行持有）。
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, T> {
        self.inner.read().expect("shared resource poisoned")
    }

    /// 独占写——返回写锁（写者互斥，对应 D8 的多写者串行）。
    pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, T> {
        self.inner.write().expect("shared resource poisoned")
    }

    /// 复制共享句柄（`Arc` 克隆——所有句柄指向同一份数据）。
    pub fn clone_handle(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(feature = "std")]
impl<T> Clone for SharedResource<T> {
    fn clone(&self) -> Self {
        self.clone_handle()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 单元测试
// ════════════════════════════════════════════════════════════════════════════

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn shared_resource_read_write() {
        let shared = SharedResource::new(0i32);
        {
            let mut w = shared.write();
            *w += 10;
        }
        assert_eq!(*shared.read(), 10);
    }

    #[test]
    fn shared_resource_multi_handle() {
        // 多个句柄共享同一份数据（共享数据原语（`Resource` 类）的受控形态）。
        let shared = SharedResource::new(vec![1i32, 2, 3]);
        let handle_a = shared.clone_handle();
        let handle_b = shared.clone_handle();

        {
            let mut w = handle_a.write();
            w.push(4);
        }
        assert_eq!(handle_b.read().len(), 4, "all handles observe the same data");
    }
}
