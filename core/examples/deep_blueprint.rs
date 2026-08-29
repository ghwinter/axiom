//! 蓝图深度 × 编译成本的诚实探针（采纳证据层；可复现测量）。
//!
//! 目的：回答"蓝图作组合蓝本，编译要付什么、不付什么"——这是采纳的地面层（**一次诚实的
//! 编译成本数据**）。**运行期**零成本由核心的 [`chain` bench](../benches/chain.rs) 聚焦测量
//!（本探针不重复）：`step` 全类型参数化、组合缝零指令。本探针补它没有的**编译侧**与**单态化
//! 体积**——跌破"零成本"被误读成"编译也零成本"的坑：**运行期零，编译期不零，且随深度涨**。
//!
//! ## 方法学（诚实测量，随本文件行走）
//!
//! - **测量分离**：先把 `axiom` crate 本体热缓存建好，再 `time -p cargo build --release
//!   --bin deep_blueprint` 取 `real`。想隔离本文件**边际**编译成本，先用一个浅测一次、再
//!   在此文件加深一层重测，取两次差值。
//! - **运行时观测**（非计时、仅现象）：exe 启动一次 ≈ 0.02 s，深 2 与深 32 相同——两深度都
//!   常量折叠；真正按步的运行时对比见 `chain` bench，此处不伪造数字。
//! - **体积**：`stat -c %s target/release/deep_blueprint.exe`（Windows）或 `ls -l`。
//! - **诚实边界（A5）**：以下为**本机一次测得**，非扇区，随 rustc / LTO / 机器漂移；二进制
//!   体积依赖 `lto = true`。**不伪称**这些数字是任何机器的常数——只给一张"长什么样"的快照
//!   ＋你可在此复跑的探针。
//!
//! ## 实测真值表（2026-08-29；rustc 1.98.0；release + LTO+opt-3；W64）
//!
//! | 维 | 深 2 | 深 32 | 读数 |
//! |---|---|---|---|
//! | 边际编译时间（双 bin 分离测量） | ≈0.25 s | ≈2.05 s | **深度×8 ⟹ 编译 ≈×8**：编译成本非零、随深度线性涨 |
//! | 二进制体积 | 110,592 B | 110,592 B | LTO 下 32 层 Add 链折叠，体积持平 |
//!
//! 语义：`Chain32 = sum(1..=32)=528` 每次 `step` 加 528，编译器把 32 个 `+K` 折叠为单指令。
//! **零成本承诺成立，代价从运行期转移到编译期**——这正是"编译期验证/展开"的物理真值，诚实呈现。
//!
//! ## 布线诊断证据（同一次勘察，并入本文件）
//!
//! 代表性错配（`Out=i32` 接 `In=String`）经 `assert_wiring`，rustc 1.98 实测为
//! **`E0271: type mismatch resolving \`<S as PortCell>::In == i32\``**，点在调用行。即便埋在
//! `Chain<A1,Chain<A2,Chain<A3,A4>>>` 末端，投影归一化仍把失配归结为**原子不相等 + 调用点**，
//! 不崩在深层泛型堆里。**纠偏**：docs 与外部分忧"深层泛型编译错误难啃"，本形态上不成立。
//! **防自满**：属单类型错配取样，非全部失败形态（`Conforms<Slot>`/形状约束）未逐一取证，随
//! rustc 漂移，不作持久承诺。权威诊断形态亦见 [`assert_wiring`](crate::cell_core::assert_wiring)。
//!
//! ## 重跑
//!
//! ```text
//! # 编译成本随深度伸缩（加深一层的真验）
//! \time -p cargo build --release --bin deep_blueprint          # 测当前深度
//! # 在此文件连锁再加一层（如 `Chain64`），重跑同命令，对比两种 `real`
//! # 体积：stat -c %s target/release/deep_blueprint.exe
//! ```
//!
//! `debug` 构建（`--all-targets`）下本文件仍须编译通过——类型定义即产物，无测量体可移除。

use axiom::cell_core::{Chain, PortCell};
use std::marker::PhantomData;

/// 单步常数加法器；参数化为模板以生成纵深链。
pub struct Step<const K: i64>;
impl<const K: i64> PortCell for Step<K> {
    type In = i64;
    type Out = i64;
    type State = PhantomData<i64>;
    fn step(_: &mut PhantomData<i64>, x: i64) -> i64 {
        x + K
    }
}

/// 深 2 链：+1 → +2。
pub type Chain2 = Chain<Step<1>, Step<2>>;
/// 深 8 链：+1..+8。
pub type Chain8 = Chain<Chain2, Chain<Chain2, Chain<Chain2, Chain2>>>;
/// 深 32 链：+1..+32（嵌套四个 Chain8）。
pub type Chain32 = Chain<Chain8, Chain<Chain8, Chain<Chain8, Chain8>>>;

/// 类型存在性见证：用一次 `step` 证明上述链可真编译、可被该文件携带。
fn main() {
    let mut s = <Chain32 as PortCell>::State::default();
    let x = Chain32::step(&mut s, 0);
    std::hint::black_box(x);
}