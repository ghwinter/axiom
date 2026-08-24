//! 统一模型（runtime 激活侧）测试：
//! - 型位的运行期存在化（SlotPending → SlotDrive：install / commit / drive / swap）
//! - 无界计数序列驱动（drive_seq）

use axiom::cell_core::PortCell;
use axiom_runtime::prelude_all::{SlotDrive, SlotPending, drive_seq};

// 测试 cell：加一（In=i32, Out=i32）。
struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s += x;
        *s
    }
}

// 测试 cell：倍乘（In=i32, Out=i32）。
struct Scaler;
impl PortCell for Scaler {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s = *s * 2 + x;
        *s
    }
}

#[test]
fn slot_drive_install_and_drive() {
    // ∃ 绑定：运行期安装一个合规居留项并驱动（类型已擦除）。
    let mut slot: SlotDrive<i32, i32> = SlotPending::install::<Inc>(0).commit();
    // Inc(0): step(0, 5) -> 5
    assert_eq!(slot.drive(5), 5);
    // 状态保持：step(5, 3) -> 8
    assert_eq!(slot.drive(3), 8);
}

#[test]
fn slot_drive_swap_existential_replacement() {
    // 运行期换装不同的合规居留项（存在化代换）。
    let mut slot: SlotDrive<i32, i32> = SlotPending::install::<Inc>(0).commit();
    assert_eq!(slot.drive(5), 5);
    // 换装 Scaler（也满足 In=i32, Out=i32），状态重置为 0。
    slot.swap::<Scaler>(0);
    // Scaler(0): step(0, 5) -> 5
    assert_eq!(slot.drive(5), 5);
}

#[test]
fn drive_seq_unbounded_count_generative() {
    // 无界计数的生成侧：运行期按序列驱动（计数由运行期决定，非编译期常量）。
    let mut st = 0i32;
    // Inc 累加：5,3,2 -> 累积 0+5=5, +3=8, +2=10
    let out: Vec<i32> = drive_seq::<Inc, _, _, _>(&mut st, vec![5, 3, 2]);
    assert_eq!(out, vec![5, 8, 10]);
    assert_eq!(st, 10); // 状态跨次保持
}
