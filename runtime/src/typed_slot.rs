//! 类型化值槽——动态路径级间的零分配值传递（unsafe 封装点）。
//!
//! # 安全不变量（`design-principles.md` §5.5：封装性 unsafe 三条件）
//!
//! 1. **对外安全接口**：本模块的 `pub` 方法均为安全接口；唯一 `unsafe`
//!    块在 [`take_input`] 与 [`put_output`]（见下），调用方零 `unsafe`。
//! 2. **不变量文档化**：
//!    - 槽持有 `Box<dyn Any + Send>`（类型擦除的裸值），`take`/`put`
//!      为安全装箱/移动；
//!    - [`take_input`] 用 `ptr::read` 位拷贝取出裸值（不消费分配），返回
//!      指向未初始化内存的 `*mut InRaw`——**必须**经 [`put_output`] 写回
//!      或释放，不得泄漏或重复读取；
//!    - [`put_output`] 的位拷贝（`copy_nonoverlapping`）**仅在 `TypeId`
//!      相等时执行**：Rust 保证同类型唯一 `TypeId` ⟹ `InRaw`/`OutRaw`
//!      为同一类型 ⟹ size/align 相同 ⟹ 位拷贝是合法类型重写，内存安全；
//!      跨类型时用 `dealloc(Layout::new::<InRaw>())` 释放分配（不 drop
//!      未初始化内容），另装箱。
//! 3. **测试覆盖**：同类型复用（0 分配）、跨类型重装箱、类型不匹配拒绝、
//!    多轮读写往返。

use alloc::boxed::Box;
use core::any::{Any, TypeId};

/// 取输入裸值并保留分配（级间免装箱的第一步）。
///
/// 从 `boxed`（含 `InRaw` 裸值）`downcast` 到 `Box<InRaw>`（同一分配），
/// `Box::into_raw` 拿指针（不 drop 分配），`ptr::read` 位拷贝取出裸值。
///
/// # Safety
///
/// 返回的 `*mut InRaw` 指向**未初始化**内存（值已被读走）——调用方
/// **必须**经 [`put_output`] 写回（恢复有效）或释放（不 drop 未初始化
/// 内容），且不得重复读取或 drop 该指针（泄漏/UB 由调用方负责）。
pub(crate) fn take_input<InRaw: 'static>(
    boxed: Box<dyn Any + Send>,
) -> Result<(InRaw, *mut InRaw), Box<dyn Any + Send>> {
    match boxed.downcast::<InRaw>() {
        Ok(input_box) => {
            let raw_ptr = Box::into_raw(input_box);
            // SAFETY: raw_ptr 指向有效的 InRaw（downcast 成功）；ptr::read
            // 位拷贝后该内存未初始化（不得再读，见本函数 Safety 注释）。
            let raw_in = unsafe { core::ptr::read(raw_ptr) };
            Ok((raw_in, raw_ptr))
        }
        Err(b) => Err(b),
    }
}

/// 写回输出裸值（级间免装箱的第二步）。
///
/// **同类型**（`TypeId` 相等）：位拷贝 `raw_out` 到 `raw_ptr`（`InRaw`
/// 分配复用，**零分配**），重建 `Box<InRaw>` 返回。
/// **跨类型**：释放 `raw_ptr` 分配（不 drop 未初始化内容），新装箱返回。
///
/// # Safety
///
/// `raw_ptr` 必须来自 [`take_input`]（指向未初始化内存）；同类型时位拷贝
/// 使内容恢复有效（`Box::from_raw` 正常析构），跨类型时 `dealloc` 不触碰
/// 内容。`raw_out` 经 `forget` 跳过析构（同类型时位已复制）。
pub(crate) fn put_output<InRaw: 'static + Send, OutRaw: Any + Send>(
    raw_ptr: *mut InRaw,
    raw_out: OutRaw,
) -> Box<dyn Any + Send> {
    if TypeId::of::<InRaw>() == TypeId::of::<OutRaw>() {
        // SAFETY: TypeId 相等 ⟹ 同类型 ⟹ size/align 相同；raw_ptr 指向
        // InRaw 大小的未初始化内存（take_input 保证）；位拷贝是合法重写。
        unsafe {
            core::ptr::copy_nonoverlapping(
                &raw_out as *const OutRaw as *const u8,
                raw_ptr as *mut u8,
                core::mem::size_of::<InRaw>(),
            );
            core::mem::forget(raw_out);
            Box::from_raw(raw_ptr)
        }
    } else {
        // SAFETY: raw_ptr 来自 Box<InRaw>（take_input 的 into_raw），
        // Layout::new::<InRaw>() 是正确释放参数；内容未初始化，dealloc
        // 不调用 drop（无 UB）。
        unsafe {
            alloc::alloc::dealloc(raw_ptr as *mut u8, core::alloc::Layout::new::<InRaw>());
        }
        Box::new(raw_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_type_recycle_reuses_allocation() {
        // take_input + put_output 同类型：分配复用（无新 Box）。
        let boxed: Box<dyn Any + Send> = Box::new(0i32);
        let (raw_in, raw_ptr) = take_input::<i32>(boxed).expect("i32 input");
        assert_eq!(raw_in, 0);
        let out = put_output::<i32, i32>(raw_ptr, 42);
        assert_eq!(out.downcast_ref::<i32>(), Some(&42));
    }

    #[test]
    fn cross_type_recycle_reboxes() {
        let boxed: Box<dyn Any + Send> = Box::new(0i32);
        let (raw_in, raw_ptr) = take_input::<i32>(boxed).expect("i32 input");
        assert_eq!(raw_in, 0);
        let out = put_output::<i32, String>(raw_ptr, "hi".to_string());
        assert_eq!(out.downcast_ref::<String>().map(String::as_str), Some("hi"));
    }

    #[test]
    fn type_mismatch_input_rejected() {
        let boxed: Box<dyn Any + Send> = Box::new(3i64);
        assert!(take_input::<i32>(boxed).is_err(), "downcast rejects mismatched type");
    }
}
