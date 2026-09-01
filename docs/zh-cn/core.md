> **语言：** 中文 · [English](../en-us/core.md)

# axiom 编译期核心：cell_core（axiom 的"应该是什么"·核心卷）

> **性质**：axiom 的核心架构规范。回答"axiom 核心是什么"：把 `foundations.md`
> 的公理与定理，落成一个编译期核心 `core/src/cell_core.rs`。本卷描述 axiom 核心
> 的形态，与已收敛的实现（`core/src/cell_core.rs`、`core/src/lib.rs`）一致。
>
> **规范性**：自洽的权威规范，专注 axiom 核心自身的定义。
>
> **摘要**：axiom 核心层 = compile-time DSL + 验证器：全部"智能"（分析、
> 验证、类型约束、图构造）在编译期耗尽；产物是普通 Rust 代码。axiom 没有
> "运行时"——只有"编译期"与"编译后"两段。这天然满足零成本承诺（编译后无 axiom
> 对象）。

---

## 1. 核心命题

> axiom 核心层 = compile-time DSL + 验证器：全部"智能"在编译期耗尽；
> 产物是普通 Rust 代码。axiom 没有"运行时"——只有"编译期"与"编译后"两段。

**含义**：蓝图是编译期构造物（类型 + const + 宏生成的代码）。验证由宏 / 类型 /
const 在编译期完成——违反蓝图规则 = 编译错误
（`compile_error!`）或类型约束失败，而非运行时 `Result`。

---

## 2. 四构件（cell_core 的主轴线）

`cell_core` 承载四个构件，对应理论收敛（衔接 `foundations.md`）：

| 构件 | 内容 | Rust 对应 | 编译期性质 |
|---|---|---|---|
| **开放系统/端口体** | 有边界、类型化输入/输出/状态，`step` 纯且可内联 | `PortCell` trait（`core/src/cell_core.rs`） | 类型级，无运行时对象 |
| **因果数据流** | 带方向的连接：`A.out -> B.in`，类型层对偶配对 | `Wire<A,B>` | 非法连接编译失败（T1） |
| **组合/嵌套** | 组合子仍是端口体，任意层级 | `Chain<A,B>` | 操作类结构（T2） |
| **静态性声明** | 标记哪些子图要求零成本 | `Static<SUB>` / `Blueprint<TOP>` | 单态化，无 `Box<dyn>`（T7/§5.6） |

**多对多通成一等**：`Broadcast`（fan-out）、`Merge`(fan-in)、`Feedback`（环）在类型层表达，
无 Tee 树（衔接 `foundations.md` §5.3/A2）。

### 2.1 `PortCell`（开放系统/端口体）

```rust
pub trait PortCell: Sized {
    type In;                      // 输入端口类型（承载的值类型）
    type Out;                     // 输出端口类型
    type State: Default;          // 内部状态
    fn step(state: &mut Self::State, input: Self::In) -> Self::Out; // 纯转移
}
```

- `In`/`Out` 是端口类型（对偶靠它们配对，见 `Wire`）；`State` 是内部状态，默认可构造；
- `step` 是纯转移（`#[inline(always)]` 使内联跨 crate 成立 → Z1 的 (b)）；
- 纯抽象层——不掺线程/同步/背压/时序，那些是物理载体的事（T3 / §5.4）。

**命名阶梯与规模中立**（规范登记；权威来源见 `foundations.md` §8.1）：

```text
数学锚：  开放系统 / 最小系统 (S, I, O, δ)、Mealy 余代数      ← 证明处使用
规范名：  端口体（中文）/ port cell（英文）                    ← 文档定义处使用
工程名：  PortCell / cell_core（冻结，不改名）
退役别名：单元、module、container、component                  ← 历史语境之外不得使用
```

**规模中立**：端口体对规模零假设——可以是数行的纯函数，也可以是数十万行子系统的边界。
内部实现（slab/arena、work-stealing、SIMD/GPU、WAL/LSM/mmap 等）归边界之内；端口体只承担
四项接缝义务：类型化端口、State 独占、全转移 δ、纯且原子的 step。配置的代数内正解＝
泛型参数化或定义期 State 覆写，不引入运行期 ConfigSchema。

### 2.2 `Wire`（因果数据流）

```rust
pub struct Wire<A, B>(PhantomData<(A, B)>);
impl<A, B> Wire<A, B>
where A: PortCell, B: PortCell<In = A::Out>,   // 类型层对偶配对
{
    pub fn fire(astate: &mut A::State, bstate: &mut B::State, input: A::In) -> B::Out {
        let mid = A::step(astate, input);
        B::step(bstate, mid)
    }
}
```

**布线合法性 = 类型判定（T1）**：要求 `B::In == A::Out`。若类型不匹配，本类型根本无法
实例化——非法连接在编译期被拒绝（不是运行时检查）。`Wire<A,B>` 是一个类型化位置
（统一 `Conforms` 对象）兼编译期绑定组合动作——即"代换绑定"概念 4 的编译期侧
（`foundations.md` §8）。

### 2.3 `Chain`（组合/嵌套）

```rust
pub struct Chain<A, B>(PhantomData<(A, B)>);
impl<A, B> PortCell for Chain<A, B>
where A: PortCell, B: PortCell<In = A::Out>,
{
    type In = A::In; type Out = B::Out; type State = (A::State, B::State);
    fn step((sa, sb): &mut (A::State, B::State), input: A::In) -> B::Out {
        let mid = A::step(sa, input);
        B::step(sb, mid)
    }
}
```

组合 A -> B（A 输出布线到 B 输入）仍是端口体，可再嵌套（任意层级）——即
`foundations.md` §8 概念 3"组合封闭"。命名 `Chain`（组合子默认名，无同义不同名）。

### 2.4 `Broadcast` / `Merge` / `Feedback`（多对多、环）

- **`Broadcast<SRC, R1, R2>`**：源输出同时布线到多个接收者（fan-out）。类型层强制所有
  接收者输入与源输出一致；无 `Box<dyn>`、无运行时对象。源输出要求 `Clone`——多路分发在
  物理层本质是复制/分发，属于物理载体的职责，抽象层只是在类型层声明"这一个值流向多个
  接收者"。
- **`Merge<S1, S2, DST>`**：多个相容源合入一个接收者（fan-in）。汇合的"顺序"（谁先到）
  是物理载体的事（T3/Kahn）——抽象层只声明"多个源可布入同一接收者"这一因果形态。
- **`Feedback<BODY, FEED>`**：`BODY` 输出经 `FEED` 回喂到 `BODY` 输入，形成因果闭合。
  **听证 D 裁定（守卫反馈）**：抽象层不只声明环的存在——`Feedback` 的单元形式即守卫：
  每个外部输入执行一次内联闭合迭代（`BODY -> FEED -> BODY`，两拍），FEED 侧一拍延迟；
  yanking（无延迟即取）不是其语义（断言对象，负见证在 core laws.rs）。环良定义义务由该
  抽象层守卫承担（T3 二次修正）；缓冲环路径的等效守卫（FIFO 即延迟）归物理载体
  （semantics `drive_feedback_inline`，Moore 门）；无缓冲内联正确性假设 `FEED` 仅依赖
  状态（Moore，声明非证明）。

### 2.5 `Static` / `Blueprint`（静态性声明 + 蓝图即类型）

```rust
pub struct Static<SUB>(PhantomData<SUB>);          // 标记子图要求零成本
pub struct Blueprint<TOP>(PhantomData<TOP>);       // 蓝图 = 零大小类型
pub const fn blueprint_is_zero_sized<TOP>() -> bool {
    core::mem::size_of::<Blueprint<TOP>>() == 0
}
```

- **`Static<SUB>`**：显式声明一个子图为静态（零成本）。仅对声明为静态的子图，编译期强制
  单态化 + 内联，验证零成本（Z ⟹ 展开）；未声明的走普通 Rust/载体路径（dynamic 税可
  接受）——"静态优先 + 显式例外"（衔接 `foundations.md` §5.6）。
- **`Blueprint<TOP>`**：一张蓝图 = 一个零大小、编译期定型的类型（类型参数集合）。
  与"值形态蓝图/JSON"相反（衔接 `foundations.md` §5.5）：蓝图不是运行时对象，而是类型
  参数集合；`size_of::<Blueprint<TOP>>() == 0`——运行时没有任何蓝图对象。

---

## 3. 蓝图即类型、无 JSON / 值形态中间层

> **结论（衔接 `foundations.md` §5.5 / 4.1）**：在编译语言（Rust）主流里，"运行时修改代码/
> 拓扑"没有必要的普遍例子，工程上明确偏向编译期。蓝图直接用 Rust 代码定义
> （类型 / 宏调用刻画静态图结构），不需要 JSON/值形态作为一等表达。

**论证**：
- 动态库加载（dlopen/.so）是"插件"常见形态，但加载的不是新代码——`.so` 是编译期已定型
  单元，加载只是"连接既有代码 + 符号绑定"，类型平面不变，落实例层。
- 真正的运行时生成新代码（JIT）才创造新类型——非编译语言主流，工程上几乎不用于可靠性
  系统。
- "配置/JSON/序列化导入"唯一价值是"修改程序行为"——而正因运行时不改结构（T9），此价值
  不存在。JSON 至多是"生成这份 Rust 代码的工具输入"，不是一等形态。

**形式化（蓝图形态）**：蓝图 G = Rust 代码定义的类型化图（开放系统 + 因果数据流 + 端口
类型），编译期直接展开；无运行时蓝图对象，无中间 JSON 形态。此前"值作为编译前源"的
保留收回——值形态连编译前中间态都不必要。

---

## 4. 编译期验证（能力到编译期耗尽）

```rust
// 对偶/代换的**统一 T1 判据**：`EXPECT` 是一个类型化位置/接口——
// - `Wire<A, B>`：一条线（期望从 A.out 流入 B.in，即 `B::In == A::Out`）；
// - `Slot<I, O>`：一个型位（期望 `In=I, Out=O` 的居留项）。
pub trait Conforms<EXPECT> { const OK: bool = true; }
impl<A, B> Conforms<Wire<A, B>> for () where A: PortCell, B: PortCell<In = A::Out> {}

pub fn assert_wiring<A, B>() where A: PortCell, B: PortCell<In = A::Out> {
    assert_conforms::<Wire<A, B>, ()>();
}
```

- **编译期对偶判定（统一）**：若 `Conforms<Wire<A,B>>` 可构造（impl 存在），则这条布线在该
  类型对偶下合法——纯类型层（T1）。同一个判据也覆盖型位合规（`Conforms<Slot<I,O>>`）。
- **断言一条布线合法**：编译期成立则产生零大小证据；若类型不配对则该 impl 不存在 →
  编译错误。这是"用于分析与验证"的入口——验证在编译期完成，运行期零开销。

---

## 4b. 验证职责边界（类型级约束 vs 宏检查）

按落位律：性质进入 trait/类型级约束，当其违反必须*不可表示*（结构见证，模态①——
如 `Conforms`、`CAP` 门、端口同构）；进入宏 emit 检查（`compile_error!`），当其可判定
但"可表示＋带诊断"比"不可表示"更可用（如蓝图 lint："每个注册型位都有居留项"）。
不可判定的性质保持声明（模态④）。宏永不*证明*——它只是把一次②/③检查前移并获得更好
的诊断；任何来自宏的"已证明"主张都是伪验证缺陷。

## 5. 理论 ↔ Rust 的对应

| 理论对象 | Rust 对应 | 代价 |
|---|---|---|
| 开放系统/端口体 | `trait`（有输入端/输出端关联类型） | 编译期 |
| 形状-内容分离 | 泛型（形状=类型参数，内容=具体实现） | 编译期单态化 |
| 连接一等对象 | 连接类型 + 会话类型（协议对偶） | 编译期；运行期为值 |
| 类型-项二分 | `Type`（静态）vs `Box<dyn ...>` 或实例（动态） | 静态零 / 动态税 |
| 组合 | 组合子/嵌套泛型 + 递归 | 编译期展开 |
| 零成本守恒 | 泛型单态化（monomorphization） | 编译期（体积换速度） |
| no_std | 无运行时依赖 | — |

**关键对应：泛型单态化 = 零成本的实现机制**
Rust 的泛型在编译期为每个具体类型生成专门代码（monomorphization）——这是零成本守恒的
机制。当拓扑编码在类型参数里（组合子、嵌套泛型），编译器展开为手写等价指令序列；类型
擦除触发时（`Box<dyn Any>`）才付费。

---

## 6. 移出抽象层的旧语义（归物理载体/实例层）

为保持核心"干净"，以下旧语义被移出抽象层（衔接 `foundations.md` §5.4/5.8）：

- **FlowKind（Data/Control/Observe 三分）**：不作为蓝图构造原语（`flow_kind` 可选化，
  `None` = 无标注）；是抽象层可选语义注解，描述接收端如何解释值——非物理载体属性
  （物理层统一为值流经结构，见 `foundations.md` §5.8）。
- **LinkKind 的载体/背压/时序语义**：物理载体的事，非抽象层。
- **值形态蓝图 / JSON / 运行时值验证**：蓝图即代码，无 JSON/值形态中间层。
- **线程/同步异步/时序**：实例物理层（T9/T3）。

4. 验证在编译期（`Conforms`/`assert_wiring`，非法布线编译失败）。
5. **移除出抽象层的旧语义**（§2 表 + §6）不进入核心类型——`cell_core` 清理后仅剩
   `PortCell` 系 + 驱动 + 编译期验证，零依赖、`#![forbid(unsafe_code)]`、`#![no_std]`。

---

## 6b. 统一模型构造子（加法式）

在四构件之外，`cell_core` 加法式地新增统一模型构造子（不改写既有类型）：

- **`Rep<N, C>`** —— 正则幂：同一 cell `C` 的 N 次自组合（恰好 N 次的 `Cⁿ`，计数以编译期常量
  有界）。`State = RepState<N,C>`（手动 `Default`，不依赖原生数组 `Default`）；零成本、
  单态化；`N=0` 恒等。无界计数（运行期）是生成/物理侧——见 `semantics.md` 的 `drive_seq`。
- **`Slot<I, O>` + `Conforms` / `assert_conforms`** —— ∃ 型位定义：编译期固定接口
  （对偶对 T1）+ 对任何未来居留项的编译期参数化合规判定
  （`∀ T: PortCell<In=I, Out=O>` ⟹ `Conforms<Slot<I,O>>`，形同统一 `Conforms`）。运行期存在化
  填充为 `SlotDrive`（概念名：存在绑定，existential binding）——见 `semantics.md`。
- **`Choice<A, B>` + `Opt<C>`** —— 正则算子 `|` 与 `?` 的一等纯 `PortCell` 表达。`Choice`
  （输入标号[和]）由输入的标签派发给 `A` 或 `B`；`Opt<C>` 把 `Option<C::In>` 映射为
  `Option<C::Out>`（`None` 恒等，`Some` 应用一次 `C::step`）。二者确定、可像普通 cell 一样
  组合（其 ∃ 分支选择侧仍是 semantics 的 `SlotDrive`）。

这些是定义（零大小、无运行时对象），复用同一套 `PortCell` + `Conforms` 式编译期验证
——统一模型静片段的加法式实现（见 [`unified.md`](unified.md)）。

### 6c. 构造子 → 构造概念 实例矩阵 与 封闭性检查清单

每个构造子都是五个构造概念（`foundations.md` §8.1）的一个实例，不是第六个概念：

| 构造子 | 它实例化的构造概念 |
|---|---|
| `PortCell` | 概念 1 —— 端口体 / 开放系统 |
| `Wire<A,B>`（+ `Conforms<Wire<..>>`） | 概念 1/2/4 —— 类型化因果流 + T1 对偶组合（编译期绑定位置） |
| `Chain` / `Rep` / `Broadcast` / `Merge` | 概念 3 —— 组合封闭（各自是端口体、可嵌套） |
| `Feedback` | 概念 3 的受守卫扩展（组合性来自 3；守卫面——一拍延迟 + yanking 负见证——是不可由其余成员导出的裁决结构，见 §2.4 与 foundations T3 二次修正） |
| `Choice` / `Opt` | 概念 1 —— 输入携带标签 / 可空的端口体（仅类型） |
| `Slot<I,O>`（+ `Conforms<Slot<..>>`） | 概念 4 —— 型位：带类型的开放位置（未绑定义） |
| `SlotDrive`（semantics；存在绑定） | 概念 4/5 —— 运行期（∃）绑定，然后激活 |
| `TryChain` / `drive_try`（semantics） | 概念 1 —— 失败为值（`Result`）经组合，保持 `step` 全函数 |
| `drive` / `drive_seq` / 载体（semantics） | 概念 5 —— 激活（运行）/ 输送（函数、缓冲、通道） |

**封闭性检查清单（源自 §8.3）**——在加任何能力 C 之前，问：
1. C 是否五个概念之一的实例（能用 `PortCell` + T1 组合 + 该/这些绑定 + 激活表达）？
   是 → 合法（是实例，不是补丁）。
2. 若 C 需要一个新的第六个构造概念（无法用 1–5 表达），则须要么拒绝、要么经集体裁定显式
   新增——不容隐性新规则（不用"五个概念之外"的新类型/特征偷偷加概念）。

---

## 7. 验收基准（核心）

```text
cargo build --lib        # 零依赖，no_std 支持（--no-default-features）
cargo test               # 21 单元测试 + 5 项封闭边界断言（tests/closed_boundary.rs）+ 6 项蓝图集成断言（tests/topology_blueprint.rs；基准不混入测试）
cargo bench --bench chain   # 静态 ≈ 手写（零成本实证，仅 release 数字有意义）
cargo bench --bench dag     # 菱形零成本实证（Δ(复合−手写)≈±1%，落在自噪声底量级内）
```

**已达成（证据链）**：
- 四构件完整、可编译、有测试；复杂拓扑（环/广播/汇合）在类型层表达，无
  `Box<dyn>`/JSON/线程/FlowKind。
- 蓝图即类型：`size_of::<Blueprint<TOP>>()==0`，运行时零对象（const 证明）。
- 验证在编译期（`Conforms` 类型判定，非法布线编译失败）。
- 编译后等价手写普通 Rust（`examples/cell_demo.rs` 实证）。
- bench（`bench_common.rs`：预热 → 轮换交错 → min-of-N + 自噪声底）：Δ(复合−手写) 实测
  −0.5% ~ +1.2%，自噪声底 ±0.1~0.6%——差异落在测量不确定度量级内、与零不可区分；早先 2.7~6.1%
  的顺序性波动系单次计时伪影，非抽象成本。type-erasure 动态税约 2.5–5×（对照）——实证
  `foundations.md` T7 的"静态免费、动态必付税"。
- `#![forbid(unsafe_code)]`、`#![no_std]`（`default=["std"]`），核心零依赖。

---

## 8. 边界与开放问题

- **核心是编译期模型**：能力到编译期耗尽，编译后无 axiom 对象；超出编译期的"智能"
  （如线性时态/图分析）不属核心默认能力，须另行设计。
- **全函数假设**：`PortCell::step` 被假定为总转移；"会失败的 cell"未公理化，当前按物理
  `Result` 约定处理（见 [`semantics.md`](semantics.md) 开放问题）。
- **静态性声明的覆盖范围**：目前 `Static` 以类型参数标记静态子图；"静态路径从链扩大到
  任意多子图"后的单态化体积容忍上限是开放问题（`foundations.md` §7）。
- **已知的最可能修正点（登记在案；由证据裁决，非先验裁定）**：`PortCell` 的单 `In`/单 `Out`
  签名。多通道模块今日已可表达且运行时零成本（`Choice` 标签化输入；仲裁 cell 形态 =
  `Merge` 扇入单一状态持有者 + `Broadcast` 扇出），但若第一个中尺度真实系统表明元组/和
  类型编码的人体工学税是常态性而非偶发性，压力将指向行类型化（多端口）cell——这是对
  构造概念 1 的宪法级修正。先行登记，使字母表最脆弱的接缝在被撞上之前有名字（审计谱系
  保存在内部的实现审计登记中，不属于本公开文档）。

> **结论**：axiom 核心 = `cell_core` 四构件（开放系统、因果数据流、组合、静态性声明）
> 的编译期模型——蓝图即类型、验证在编译期、编译后等价手写普通 Rust。物理实现由
> 语义层（载体）承担，见 [`semantics.md`](semantics.md)。
