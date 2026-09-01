//! 枚举式型位（编译期已知候选集；A4）——候选集已知时的零擦除存在化。
//!
//! 与 [`Slot`](axiom::cell_core::Slot)/`SlotDrive`（`dyn` 擦除，运行时任选居留项）
//! 相对，本形态的候选集 编译期已知（二元 `{A, B}`）：驱动 = 一次索引 match，
//! 无 `downcast`、无装箱。两条成本曲线（外部审计 A4）：
//!
//! | 形态 | 成本曲线 | 代价 |
//! |---|---|---|
//! | `Slot<I,O>` + `SlotDrive` | 每安装/换装一次堆分配 + 函数指针间接 + 代戳校验；任意居留项 | 动态税（C9 实测） |
//! | [`EnumSlot`] | 驱动零分配零擦除；候选状态常驻双份（内存 2×） | 候选集必须编译期定死 |
//!
//! **概念归属（§8.3）**：概念 4（型位）的编译期候选变体——选择是"运行期在
//! 已知集合中取一"，非新概念；以 `State` 承载选择位（bool），构造总保持 total
//! （无 `init`，符合 C15-T1 的构造偏好）。
//!
//! **no_std + alloc 可用**：本模块无 `std` 依赖。

use axiom::cell_core::PortCell;

/// 枚举式型位：在编译期已知候选集 `{A, B}` 间运行期选择（零擦除）。
///
/// `State = (bool, A::State, B::State)`：选择位 + 双候选状态（常驻；切换不丢
/// 任一候选的内部状态——换回时原状继续）。
pub struct EnumSlot<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B> PortCell for EnumSlot<A, B>
where
    A: PortCell,
    B: PortCell<In = A::In, Out = A::Out>,
{
    type In = A::In;
    type Out = A::Out;
    type State = (bool, A::State, B::State);
    #[inline(always)]
    fn step((sel, sa, sb): &mut Self::State, input: A::In) -> A::Out {
        if *sel {
            B::step(sb, input)
        } else {
            A::step(sa, input)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Inc;
    impl PortCell for Inc {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x.wrapping_add(1)
        }
    }

    struct Double;
    impl PortCell for Double {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x.wrapping_mul(2)
        }
    }

    #[test]
    fn enum_slot_selects_between_candidates_zero_erase() {
        // 运行期选择：sel=false → A（Inc），sel=true → B（Double）。
        let mut st: <EnumSlot<Inc, Double> as PortCell>::State = (false, (), ());
        assert_eq!(
            <EnumSlot<Inc, Double> as PortCell>::step(&mut st, 5),
            6,
            "候选 A：Inc(5)=6"
        );
        st.0 = true;
        assert_eq!(
            <EnumSlot<Inc, Double> as PortCell>::step(&mut st, 5),
            10,
            "候选 B：Double(5)=10（同型位、零擦除切换）"
        );
    }

    struct Counter;
    impl PortCell for Counter {
        type In = i32;
        type Out = i32;
        type State = i32;
        #[inline(always)]
        fn step(s: &mut i32, x: i32) -> i32 {
            *s += x;
            *s
        }
    }

    struct Sink;
    impl PortCell for Sink {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x
        }
    }

    #[test]
    fn enum_slot_keeps_both_candidate_states_across_switches() {
        // 状态隔离：切到 B 再切回 A，A 的状态连续（常驻双份，不丢不重算）。
        let mut st: <EnumSlot<Counter, Sink> as PortCell>::State = (false, 0, ());
        assert_eq!(<EnumSlot<Counter, Sink> as PortCell>::step(&mut st, 10), 10);
        assert_eq!(<EnumSlot<Counter, Sink> as PortCell>::step(&mut st, 5), 15);
        st.0 = true; // 切到 B
        assert_eq!(<EnumSlot<Counter, Sink> as PortCell>::step(&mut st, 7), 7);
        st.0 = false; // 切回 A：原状继续
        assert_eq!(
            <EnumSlot<Counter, Sink> as PortCell>::step(&mut st, 1),
            16,
            "A 状态跨切换保留（15+1=16）"
        );
    }

    #[test]
    fn enum_slot_conforms_to_its_dual_pair() {
        // 与 Slot 同型位：conforms 判定自动成立（类型层）。
        use axiom::cell_core::{Slot, assert_conforms};
        assert_conforms::<Slot<i32, i32>, EnumSlot<Inc, Double>>();
    }
}