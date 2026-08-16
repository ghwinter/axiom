# Adapter 生态规则与 Runtime Contract 认证（M4 + M5）

> **性质**：正式文档（`docs/`）。定义第三方 runtime adapter 的生态规则
> （M4）与能力认证门槛（M5）——axiom core 是纯契约层，物理执行由
> adapter 提供；本文档规定 adapter 如何与 core 协作、如何声明能力、
> 如何被验证。

---

## 一、Adapter 生态规则（M4）

### 1.1 定位

`axiom` core 是**契约层**（端口、拓扑、验证、静态路径组合子）；物理执行
（线程、channel、IO 复用、调度）由 **runtime adapter** 提供。core 自带一个
参考 adapter（`axiom-runtime`），第三方可提供适配不同物理世界的 adapter
（异步运行时、IO 多路复用、嵌入式、WASM 等）。

### 1.2 依赖方向（铁律）

> **Adapter 依赖 core 契约，不依赖彼此 provider。**

- Adapter 的 `Cargo.toml` 依赖 `axiom`（契约层），**不依赖**其他 adapter。
- Adapter 之间通过 core 的契约（`Machine`/`LinkKind`/`ExecutionHint`/
  `RuntimeContract`）协作，不互相 import。
- 理由：adapter 是可替换的物理实现——若 A 依赖 B 的 provider，则 A 无法
  在 B 缺席时部署，可替换性被破坏。

### 1.3 分组与命名

```text
axiom/<adapter-name>           # 建议 workspace 布局
  core/                        # 契约层（唯一被依赖者）
  axiom-<adapter>/             # 第三方 adapter（依赖 core，不依赖彼此）
```

命名：`axiom-<adapter>`（如 `axiom-tokio`、`axiom-io-uring`、`axiom-wasi`）。
每个 adapter 在 `Cargo.toml` 声明其支持的 `Guarantees`（见 §二）。

### 1.4 Release expectation 分层

| 层 | 含义 | 兼容性承诺 |
|---|---|---|
| **Product** | 生产可用，稳定 API | 语义化版本，破坏性变更走 deprecation |
| **POC** | 验证可行性的原型 | 不承诺稳定，可随时重构 |
| **Support** | 测试/工具基础设施 | 低兼容期望 |

core 与参考 adapter 为 Product；实验性 adapter（如特定 IO 后端）为 POC。
新 adapter 起步可标 POC，成熟后升 Product。

---

## 二、Runtime Contract 认证门槛（M5）

### 2.1 问题

蓝图（`DeploySpec`）声明了拓扑与物理需求（`LinkKind`、`ExecutionHint`、
`MachinePhysicalSpec`）。但 adapter 的物理能力可能**不能兑现**这些声明——
例如：一个不支持 `Inline` 载体的 adapter 遇到 `Inline` 链接，一个零延迟
adapter 遇到需要 Moore 断环的拓扑。若不在部署前发现，问题在运行时才暴露。

### 2.2 机制：`RuntimeContract` + `Guarantees`

core 定义 [`RuntimeContract`]（`src/runtime_contract.rs`）：adapter 声明
其物理能力为 [`Guarantees`]——支持哪些 `LinkKind`、哪些 `ExecMode`、
内存序、IO 能力、链接延迟模型、物理预算。

**认证流程**（部署前，`materialize` 时或蓝图验证后）：

```text
DeploySpec（声明 what）           Guarantees（adapter 的 how 能力）
        │                                  │
        └────────── 审计 ──────────────────┘
        RuntimeContract::audit(spec, guarantees)
        │
        ├─ 可兑现 → 物化执行
        └─ 不可兑现 → 部署前拒绝（结构化错误，指明哪条声明无法兑现）
```

**拒绝规则**：
- 蓝图引用的 `LinkKind` 不在 adapter 的 `LinkKindSupport` 中 → 拒绝；
- 蓝图要求的 `ExecMode`（如 `Parallel(n)`）不在 `ExecModeSupport` 中 → 拒绝；
- 链接的物理预算（`PhysicalBudget`）超过 adapter 能力 → 拒绝；
- 环上无 Moore 且 adapter 是零延迟模型（`LinkDelay::Zero`）→ 拒绝
  （代数环）。

### 2.3 Reference adapter 的契约

`axiom-runtime` 实现 `RuntimeContract`（`ReferenceRuntime`），声明其
`Guarantees`：全部 `LinkKind`、`Inline`/`Sequential`/`Parallel` 执行模式、
channel 延迟模型等。它同时是第三方 adapter 的**参照实现**——照它声明
`Guarantees` 即可被 `audit` 校验。

### 2.4 对第三方 adapter 的要求

1. 实现 `RuntimeContract`，诚实声明 `Guarantees`（不支持的不声明）；
2. `materialize` 时调用审计，不可兑现即在部署前拒绝（fail loud）；
3. 声明的能力**必须被测试覆盖**（POC 层至少冒烟测试）。

---

## 三、与现有结构的衔接

- `src/runtime_contract.rs`：`RuntimeContract` trait + `Guarantees` +
  `ReferenceRuntime`（核心已就位，第三方按此实现）。
- `axiom-runtime`：参考 adapter，`materialize` 时调用 `validate` +
  端点校验（`RuntimeContract::audit` 的接入点在 `materialize` 扩展）。
- `docs/design-principles.md` §1：物理 = 有限执行形态集合——`Guarantees`
  正是该集合的类型化声明：**adapter 声明它支持集合中的哪些成员，蓝图
  声明需要哪些成员，部署时匹配，不匹配即拒绝。**

---

> **一句话**：M4 让 adapter 生态可替换（只依赖 core 契约、不依赖彼此）、
> 可分层（Product/POC/Support）；M5 让 adapter 能力可验证（`Guarantees`
> 声明 + 部署前审计，不可兑现即拒绝）。两者共同实现"deploy-time physics"
> 的工程化：**物理能力是声明的事实，部署时匹配，不匹配是配置错误而非
> 运行时事故。**
