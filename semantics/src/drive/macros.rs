//! `wire!` 声明宏——编译期展开的"连线 + 载体 + 验证"一次完成。
//!
//! 兑现目标"用宏/编译期技巧实现"：宏在编译期展开为直接的内联调用
//! （零宏运行时开销），并隐式完成布线类型判定（`A::Out == B::In`，
//! 编译期失败即编译错误，对应 cell_core 的统一 `Conforms` 判据）。

/// 声明一条因果流 `source -> sink`，返回一个驱动闭包 `F: Fn(&mut A::State, &mut B::State, A::In) -> B::Out`。
///
/// 默认用 [`InlineCarrier`](crate::movers::carrier::InlineCarrier)（栈上直接传，零分配内联）。
/// 宏在编译期展开为 `B::step(&mut *, A::step(&mut *, input))` —— 即手写等价（T7）。
/// `A`/`B` 由 `source`/`sink` 类型指定。
///
/// **路径契约**：宏展开中的 `::axiom::` 路径解析于调用方的 extern prelude，故调用方的
/// 依赖必须以字面名 `axiom` 出现（重命名依赖会在此宏的每个使用点编译失败）；axiom
/// 核心自身经 `extern crate self as axiom;` 使该路径在 crate 内亦可解析。
#[macro_export]
macro_rules! wire {
    // source: A, sink: B —— 生成一个内联驱动闭包。
    ($source:ty => $sink:ty) => {
        |sa: &mut <$source as ::axiom::cell_core::PortCell>::State,
         sb: &mut <$sink as ::axiom::cell_core::PortCell>::State,
         input: <$source as ::axiom::cell_core::PortCell>::In|
         -> <$sink as ::axiom::cell_core::PortCell>::Out {
            // 编译期布线验证：sink 输入须 == source 输出（不满足则编译失败）。
            // 通过类型约束强制（B::In = A::Out），并用统一 Conforms 判据见证（bool）。
            let _: bool = <() as ::axiom::cell_core::Conforms<
                ::axiom::cell_core::Wire<$source, $sink>
            >>::OK;
            // 内联直接传（等价手写）：
            let mid: <$source as ::axiom::cell_core::PortCell>::Out =
                <$source as ::axiom::cell_core::PortCell>::step(sa, input);
            <$sink as ::axiom::cell_core::PortCell>::step(sb, mid)
        }
    };
}
