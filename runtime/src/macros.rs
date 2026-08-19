//! `wire!` 声明宏——编译期展开的"连线 + 载体 + 验证"一次完成。
//!
//! 兑现目标"用宏/编译期技巧实现"：宏在**编译期**展开为直接的内联调用
//! （零宏运行时开销），并隐式完成布线类型判定（`A::Out == B::In`，
//! 编译期失败即编译错误，对应 cell_core 的 `DoesWire`）。

/// 声明一条因果流 `source -> sink`，返回一个驱动闭包 `F: Fn(&mut A::State, &mut B::State, A::In) -> B::Out`。
///
/// 默认用 [`InlineCarrier`](crate::carrier::InlineCarrier)（栈上直接传，零分配内联）。
/// 宏在编译期展开为 `B::step(&mut *, A::step(&mut *, input))` —— 即手写等价（T7）。
/// `A`/`B` 由 `source`/`sink` 类型指定。
#[macro_export]
macro_rules! wire {
    // source: A, sink: B —— 生成一个内联驱动闭包。
    ($source:ty => $sink:ty) => {
        |sa: &mut <$source as ::axiom::cell_core::PortCell>::State,
         sb: &mut <$sink as ::axiom::cell_core::PortCell>::State,
         input: <$source as ::axiom::cell_core::PortCell>::In|
         -> <$sink as ::axiom::cell_core::PortCell>::Out {
            // 编译期布线验证：sink 输入须 == source 输出（不满足则编译失败）。
            // 通过类型约束强制（B::In = A::Out），并用 DoesWire 见证（bool）。
            let _: bool = <() as ::axiom::cell_core::DoesWire<$source, $sink>>::WIRES;
            // 内联直接传（等价手写）：
            let mid: <$source as ::axiom::cell_core::PortCell>::Out =
                <$source as ::axiom::cell_core::PortCell>::step(sa, input);
            <$sink as ::axiom::cell_core::PortCell>::step(sb, mid)
        }
    };
}
