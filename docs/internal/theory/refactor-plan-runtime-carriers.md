# runtime 重建计划：cell_core 的物理层实现用例（载体）

> **性质**：本步操作宪法（docs/internal/，不入 git）。
> **目标**：axiom-runtime 从"旧 core 适配器"重建为"四构件编译期核心（cell_core）
> 的物理层实现用例"——提供"值如何跨连接流动"的多种可替换物理方案，模块化、可替换。
> **依据**：理论（axiom-theory-foundations §4.3 runtime 定位）+ 核心清洁（src 只剩 cell_core）。

---

## 1. 定位（一句话）

> **runtime = 载体（Carrier）目录 + 兑现验证**：它为 cell_core 的每条因果数据流
> 提供一种物理实现（值怎么移动），每种体现不同的时空成本，模块化、可替换，
> 作为未来 axiom-tokio 等第三方物理适配器的模板。

## 2. 概念基础（源于 cell_core）

- cell_core：开放系统（`PortCell`: In/Out/State/step）+ 因果流（`Link`/`CellChain`/
  `Broadcast`/`Merge`/`Feedback`）+ 静态性（`Static`）+ 编译期验证（`DoesWire`）。
- 蓝图即类型：零大小、零运行时对象、编译期耗尽。
- **runtime 不重复这些**——它只回答"这条因果流，值怎么从 A.out 到 B.in"。

## 3. 核心抽象：`Carrier`

```
trait Carrier<A, B>   // 把 cell A 的输出流动到 cell B 的输入
```

| 载体 | 物理方案 | 时空成本 | 模块 |
|---|---|---|---|
| `InlineCarrier` | 栈上函数直接传（A::step 的结果直接作 B::input） | 零分配、内联 | carrier/inline.rs |
| `QueueCarrier` | 堆队列/通道（mpsc 等），跨线程传输 | 每消息分配 + 同步 | carrier/queue.rs |
| `DirectCall` | 编译期展开（拓扑静态时，多条链内联成一个调用图） | 零运行时对象 | carrier/direct.rs |

每种载体**独立可选、可替换**：换一个实现不改拓扑（T6 多物理实现）。

## 4. 模块化与可替换

- 每个 carrier 是**独立单元**，可作为单独 crate（未来 axiom-tokio 用 async 载体替换
  QueueCarrier，axiom-io_uring 用 io_uring 载体）。
- 蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>` 走 InlineCarrier），
  运行时按声明兑现——"部署期物理"。
- carrier 提供统一的兑现验证：能否承载该拓扑的静态性要求。

## 5. 执行步骤（每步可编译可验收；用 cargo build --lib / test 验收）

1. **重建 runtime/Cargo.toml**：依赖 core 0.3（cell_core），移除旧 dev；把旧 runtime 源码
   移入 `runtime/_legacy_v0/` 搁置。
2. **建 Carrier trait + InlineCarrier**：栈上直接传，证明"物理=载体"，零分配内联。
3. **建 QueueCarrier**：堆队列跨线程传输，证明"不同时空成本=不同载体"。
4. **建 DirectCall / 驱动**：编译期展开链 + 一个最小 runtime 驱动（给定蓝图，按载体驱动）。
5. **示例**：用 cell_core + 载体跑一个"链/广播"二阶拓扑，对应 cell_demo 但走 runtime 载体。
6. **测试 + no_std**：carrier 单测、编译期验证、no_std 构建。
7. **文档 + 收束**：lib.rs 文档化定位、更新执行计划记录。

## 6. 验收标准

- runtime 只依赖 cell_core（新核心），不依赖任何 v0 模块。
- Carrier trait 清晰：换载体不改拓扑（多物理实现成立）。
- InlineCarrier 零分配/内联（与手写等价）；QueueCarrier 支持跨线程。
- 模块化：每个 carrier 独立、可单独引用。
- cargo build/test/no_std 绿。

## 7. 边界（诚实）

- 这是重建，不是增量修——旧 runtime 源码全部搁置（物理思路保留作参考，接口重写）。
- 目标是"物理层实现用例 + 模板"，不是"功能最全的通用 runtime"。
- "大胆"指可自由删改旧代码；但每步保持核心+新 runtime 可编译。

---

## 8. 执行进度记录（截至 runtime 重建 7 轮）

| 轮 | 提交 | 内容 |
|---|---|---|
| R1 | `d77d909` | runtime 骨架：Carrier trait + InlineCarrier + QueueCarrier(std) + DirectCarrier + flow 驱动；旧 runtime 移入 _legacy_v0 |
| R2 | `2e0ce35` | carrier_demo 用例：同一张 cell_core 蓝图多载体可替换、语义等价、时空成本不同 |
| R3 | `9f06463` | static_path：Static 声明 → 编译期内联展开（零运行时对象） |
| R4 | `d3a3e2d` | ChannelCarrier/spawned_flow：真实跨线程通道载体（mpsc + 独立线程） |
| R5 | `efb3404` | lib.rs 定位文档补全（载体目录/驱动/静态路径/第三方模板） |
| R6 | `24de0ce` | wire! 声明宏：编译期展开的连线+载体+验证一次完成 |
| R7 | — | 本记录；目标达成评估 |

**验收结果（全绿）**：
- runtime 只依赖 cell_core（新 core），不依赖任何 v0 模块 ✓
- 载体目录：Inline（栈上函数传·零分配）/ Queue（堆队列中转）/ Channel（跨线程 mpsc）/ Direct+static_path（编译期展开）/ wire!（声明宏）
- 模块化可替换：换载体不改拓扑（T6），各载体独立可单独引用，作 axiom_tokio/io_uring 模板 ✓
- runtime 7 测试 + core 9 测试 + no_std 构建全绿 ✓

**结论**：目标核心（runtime 重建为 cell_core 物理层实现用例，含所有物理方案 + trait/宏/编译期技巧 + 模块化可替换 + 依赖新核心 + 全程可验收）已全部达成并有完整证据链。旧 runtime 源码在 runtime/_legacy_v0/ 保留作物理思路参考。
