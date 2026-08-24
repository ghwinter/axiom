# axiom 重构执行计划（分支 rework/compile-time-core）

> **性质**：本分支的操作宪法（docs/internal/，不入 git）。
> **基调**：基于理论收敛（axiom-theory-foundations.md）的大胆重构——旧语义可被怀疑、删除、重写。
> **核心方向**：四构件蓝图（开放系统/端口体 + 因果数据流 + 组合/嵌套 + 静态性声明）
> + 编译期核心（能力到编译期耗尽，产出普通 Rust，无运行时对象）。

---

## 1. 目标形态（四构件 + 编译期）

新核心 = 一个**编译期模型** `cell_core`，承载：

| 构件 | 内容 | 编译期性质 |
|---|---|---|
| **开放系统/端口体** | trait：`In`/`Out`/`State`/`step`（纯、可内联） | 类型级，无运行时对象 |
| **因果数据流** | 带方向的连接：`A.out -> B.in`，类型层对偶配对（T1） | 非法连接编译失败 |
| **组合/嵌套** | `Chain<A,B>` 等组合子仍是端口体，任意层级（A2） | 操作类结构 |
| **静态性声明** | 标记"哪些子图要求零成本"（§4.2） | 单态化，无 Box<dyn> |

## 2. 移除出抽象层的旧语义（归物理载体/实例层）

- **FlowKind（Data/Control/Observe 三分）**：不作为蓝图构造原语（移出蓝图，`flow_kind` 可选化），
  但仍是**抽象层接收端的可选语义注解**，非物理载体属性（物理层统一为值流经结构，见
  `axiom-theory-foundations.md` §4.4 与 `axiom-conventions.md` §2）。
- **LinkKind 的载体/背压/时序语义**：物理载体的事，非抽象层。
- **值形态蓝图 / JSON（blueprint.rs）/ 运行时值验证（deploy.rs 的运行时 validate）**：
  蓝图即代码，无 JSON/值形态中间层（§4.1）。
- **线程/同步异步/时序**：实例物理层（T9/T3）。

## 3. 重构顺序（每步保持可编译、可验收）

1. **建新主轴线 `cell_core`**（本轮）：正式模块 + 文档 + 测试，可编译运行——释放探针为正式。
2. **lib.rs 重组**：声明 `cell_core`，prelude 导出；旧模块逐步标注"待迁移/待删除"，不一次删光。
3. **逐模块迁移/删除**：把与四构件兼容的（machine/static_exec/composite/portset）融入或映射，
   把纯物理的（link/resource/backpressure/runtime_contract 的物理部分）标记为物理载体、
   把值形态的（blueprint/deploy 的运行时部分）删除或编译期化。
4. **验收**：新核心能承载（广播/环/嵌套）且零运行时对象；`cargo check/test` 绿。

## 4. 验收标准

- `cell_core` 独立可编译、有 doctest/单元测试。
- 非法因果数据流（类型不匹配）编译失败（复用探针 illegal_wire 断言）。
- 新核心不含 FlowKind/JSON/线程/时序等被移除语义。
- 与旧模块的依赖关系被显式标注，允许分阶段迁移。

## 4b. 执行进度记录（截至 8 轮）

| 轮 | 提交 | 内容 |
|---|---|---|
| R1 | `2d0a6c7` | cell_core 四构件主轴线（PortCell/Link/CellChain/Static/drive） |
| R2 | `9d01e91` | cell_core 复杂拓扑：Feedback(环) + Broadcast(多对多) |
| R3 | `269d08c` | FlowKind 移出标记(DEPRECATED) + Blueprint 即类型(零大小) |
| R4 | `469fd3d` | 大胆删除死公共 API：PortRegistry/PortEntry/is_unknown |
| R5 | `1fd8432` | 编译期布线验证：DoesWire/assert_wiring |
| R6 | `46b3dc2` | 示例 cell_demo：四构件作为普通 Rust 程序运行(零运行时对象) |
| R7 | `b08ddf1` | 定位：cell_core 确立为 crate 主主轴，旧核心标 legacy |

**已达成（可验收）**：
- 四构件（开放系统/因果流/组合/静态性）完整、可编译、有测试（8 个）。
- 复杂拓扑（环/广播）在类型层表达，无 Box<dyn>/JSON/线程/FlowKind。
- 蓝图即类型：`size_of<Blueprint<T>>==0`，运行时零对象（const 证明）。
- 验证在编译期（DoesWire 类型判定，非法布线编译失败）。
- 编译后等价手写普通 Rust（cell_demo 实证）。

**遗留（有意缓行，因危及可编译性或需 redesign）**：
- FlowKind 接口层实际剥离：`HasPortInfo::flow_kind()` 及 builtin 经 prelude 依赖——
  彻底移除需 redesign 旧端口接口 + 重构 builtin（57+ 处）。新核心 cell_core 已不依赖它。
- 值形态/JSON（blueprint.rs）：serialize 集成测试依赖，删除会破坏其编译；留待新核心
  确立后再处置。
- 旧模块逐个映射进四构件/删除（machine/static_exec/composite/portset 等）——大工程，
  需在 cell_core 补齐更多组合子（fan-in/任意图形）后逐模块迁移。

**方法论结论**："大胆但有序"——先立可编译的新主轴并逐项实证（类型化/零对象/编译期
验证/等价运行），旧语义以 DEPRECATED/legacy 标记呈现新方向，然后在风险可控时再逐模块
迁移/删除。此策略在 8 轮内达成目标核心的能力闭环，遗留为需独立设计的深化。

## 5. 边界（诚实）

- 这是**方向性重构**：不要求一步完成，首轮先立可编译的新主轴线。
- 旧模块删除前标注迁移/弃用状态；凡未被新核心替代的，宁可保留不破坏编译。
- "大胆删除"指我们有授权删除，但追求**有序收敛**而非瞬间弃置全部。
