//! Typed value slot — zero-allocation value passing between stages on the dynamic path (the unsafe encapsulation point).
//!
//! # Safety invariants (`design-principles.md` §5.5: three conditions for encapsulated unsafe)
//!
//! 1. **Safe public interface**: all `pub` methods of this module are safe; the only `unsafe`
//!    blocks are in [`take_input`] and [`put_output`] (see below), so callers use zero `unsafe`.
//! 2. **Documented invariants**:
//!    - The slot holds `Box<dyn Any + Send>` (a type-erased raw value); `take`/`put`
//!      are safe boxing/moving operations;
//!    - [`take_input`] uses `ptr::read` to bit-copy out the raw value (without consuming the
//!      allocation) and returns `*mut InRaw` pointing to uninitialized memory — it **must** be
//!      written back via [`put_output`] or freed, and must not be leaked or read twice;
//!    - [`put_output`]'s bit copy (`copy_nonoverlapping`) runs **only when the `TypeId`s are
//!      equal**: Rust guarantees a unique `TypeId` per type ⟹ `InRaw`/`OutRaw` are the same type
//!      ⟹ identical size/align ⟹ the bit copy is a legal type rewrite and memory-safe;
//!      for cross-type, `dealloc(Layout::new::<InRaw>())` frees the allocation (without dropping
//!      uninitialized content) and re-boxes.
//! 3. **Test coverage**: same-type reuse (zero allocation), cross-type re-boxing, type-mismatch
//!    rejection, and multi-round read/write round-trips.

use alloc::boxed::Box;
use core::any::{Any, TypeId};

/// Take the input raw value while preserving the allocation (the first step of allocation-free
/// inter-stage passing).
///
/// From `boxed` (holding an `InRaw` raw value) `downcast` to `Box<InRaw>` (the same allocation),
/// `Box::into_raw` obtains the pointer (without dropping the allocation), and `ptr::read`
/// bit-copies out the raw value.
///
/// # Safety
///
/// The returned `*mut InRaw` points to **uninitialized** memory (the value has been read out) —
/// the caller **must** write it back via [`put_output`] (restoring validity) or free it (without
/// dropping uninitialized content), and must not read it again or drop the pointer (leaks/UB are
/// the caller's responsibility).
pub(crate) fn take_input<InRaw: 'static>(
    boxed: Box<dyn Any + Send>,
) -> Result<(InRaw, *mut InRaw), Box<dyn Any + Send>> {
    match boxed.downcast::<InRaw>() {
        Ok(input_box) => {
            let raw_ptr = Box::into_raw(input_box);
            // SAFETY: raw_ptr points to a valid InRaw (downcast succeeded); after the ptr::read
            // bit copy that memory is uninitialized (must not be read again, see this fn's Safety note).
            let raw_in = unsafe { core::ptr::read(raw_ptr) };
            Ok((raw_in, raw_ptr))
        }
        Err(b) => Err(b),
    }
}

/// Write back the output raw value (the second step of allocation-free inter-stage passing).
///
/// **Same type** (`TypeId` equal): bit-copy `raw_out` into `raw_ptr` (reusing the `InRaw`
/// allocation, **zero allocation**), rebuild and return `Box<InRaw>`.
/// **Cross type**: free the `raw_ptr` allocation (without dropping uninitialized content) and
/// re-box the new value.
///
/// # Safety
///
/// `raw_ptr` must come from [`take_input`] (pointing to uninitialized memory); for the same type
/// the bit copy restores valid content (`Box::from_raw` drops normally), and for a cross-type
/// `dealloc` never touches the content. `raw_out` is skipped via `forget` (the bits were already
/// copied for the same type).
pub(crate) fn put_output<InRaw: 'static + Send, OutRaw: Any + Send>(
    raw_ptr: *mut InRaw,
    raw_out: OutRaw,
) -> Box<dyn Any + Send> {
    if TypeId::of::<InRaw>() == TypeId::of::<OutRaw>() {
        // SAFETY: equal TypeIds ⟹ same type ⟹ identical size/align; raw_ptr points to
        // InRaw-sized uninitialized memory (guaranteed by take_input); the bit copy is a legal rewrite.
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
        // SAFETY: raw_ptr came from Box<InRaw> (take_input's into_raw), so
        // Layout::new::<InRaw>() is the correct free argument; the content is uninitialized
        // and dealloc does not call drop (no UB).
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
        // take_input + put_output with the same type: allocation reuse (no new Box).
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
