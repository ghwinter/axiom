//! 复合 Machine——子拓扑封装为单一 `machine_type`。
//!
//! # 定位（反窄化规则下的归位）
//!
//! 复合 Machine 是**结构定义能力**：一个 `DeploySpec`（子拓扑）+ 端口映射表。
//! 它属于 axiom core 的结构层，**不是** runtime 的执行能力——因为：
//!
//! 1. `CompositeSpec` 是纯数据（`DeploySpec` + 两个映射表）；
//! 2. `expand_composites` 是纯数据变换（`DeploySpec → DeploySpec`）；
//! 3. 展开过程不依赖任何执行原语（无线程、无 channel、无 reactor）。
//!
//! 把它放在 runtime 会让 core 无法独立表达嵌套拓扑——违反反窄化规则
//! （`docs/philosophy.md` §"The structural scope constraint"）的"单一功能
//! 窄化"禁令。本模块将其归位到 core，使 core 能独立定义任意深度的嵌套拓扑。
//!
//! # 设计
//!
//! 一个复合 Machine 是一个 `DeploySpec`（子拓扑）+ 端口映射表。
//! 注册为 `machine_type` 后，`expand_composites` 遇到该类型的实例时：
//!
//! 1. 展开子机器——名字空间化为 `parent.sub`（避免命名冲突）；
//! 2. 展开子链接——两端机器名加 `parent.` 前缀；
//! 3. 重定向外部链接——指向复合实例的链接按端口映射表改指向子机器。
//!
//! 嵌套（复合中的子机器也是复合）通过循环展开处理——直到无复合残留。
//!
//! # 端口映射
//!
//! - `input_map`：外部输入端口名 → (子机器名, 子端口名)
//! - `output_map`：外部输出端口名 → (子机器名, 子端口名)
//!
//! 外部链接 `(src, sport) → (comp, in_port)` 中 `in_port` 命中 `input_map` 时，
//! 改写为 `(src, sport) → (comp.sub_machine, sub_port)`。输出侧同理。
//!
//! # 与融合的关系
//!
//! 展开是结构层操作——发生在任何物化、端点校验、融合之前。融合看到的是
//! 展开后的扁平拓扑，复合的边界已消失。这使得 `FusedPipeline` 可以跨原
//! 复合边界融合（如果子机器是 `FusedInline` + `Inline` 链接）。

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
#[cfg(not(feature = "std"))]
use alloc::format;
use crate::deploy::{DeploySpec, MachineInstance};
use crate::link::LinkSpec;

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ════════════════════════════════════════════════════════════════════════════
// Section 1: CompositeSpec — 纯数据结构
// ════════════════════════════════════════════════════════════════════════════

/// 复合 Machine 定义——子拓扑 + 端口映射。
///
/// 这是**结构层**对象：它只描述"这个复合类型长什么样"，不包含任何执行
/// 逻辑。一个 `CompositeSpec` 注册为 `machine_type` 后，
/// [`expand_composites`] 会把该类型的实例替换为展开后的子拓扑。
///
/// # 验证
///
/// 调用 [`validate`](Self::validate) 检查端口映射完整性：
/// - `input_map` / `output_map` 引用的子机器必须在 `spec.machines` 中；
/// - 引用的子端口理想情况下应存在于该子机器的 `PortSchema`（需要
///   runtime 提供 schema，core 层只检查机器名存在性）。
///
/// # 序列化
///
/// 在 `serialize` feature 下，`CompositeSpec` 可 round-trip 通过 Serde，
/// 与 `DeploySpec` 一致——支持从 TOML/JSON 配置文件加载嵌套拓扑。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct CompositeSpec {
    /// 子拓扑（机器 + 链接 + funcs + settings）。
    pub spec: DeploySpec,
    /// 外部输入端口 → (子机器名, 子端口名)。
    pub input_map: BTreeMap<String, (String, String)>,
    /// 外部输出端口 → (子机器名, 子端口名)。
    pub output_map: BTreeMap<String, (String, String)>,
}

impl CompositeSpec {
    /// 创建复合定义——子拓扑 + 空端口映射（后续用 `with_input`/`with_output` 填充）。
    pub fn new(spec: DeploySpec) -> Self {
        Self {
            spec,
            input_map: BTreeMap::new(),
            output_map: BTreeMap::new(),
        }
    }

    /// 声明一个外部输入端口的映射：`ext_port` → `(sub_machine, sub_port)`。
    pub fn with_input(mut self, ext_port: &str, sub_machine: &str, sub_port: &str) -> Self {
        self.input_map.insert(
            ext_port.to_string(),
            (sub_machine.to_string(), sub_port.to_string()),
        );
        self
    }

    /// 声明一个外部输出端口的映射：`ext_port` → `(sub_machine, sub_port)`。
    pub fn with_output(mut self, ext_port: &str, sub_machine: &str, sub_port: &str) -> Self {
        self.output_map.insert(
            ext_port.to_string(),
            (sub_machine.to_string(), sub_port.to_string()),
        );
        self
    }

    /// 验证端口映射的**机器名存在性**。
    ///
    /// 这是 core 层能做的完整检查——`input_map` / `output_map` 引用的
    /// `sub_machine` 必须在 `spec.machines` 中存在。端口名存在性需要
    /// `PortSchema`（由 runtime 提供），不在 core 层检查。
    ///
    /// # 错误
    ///
    /// - [`CompositeError::DanglingInputMapping`]：`input_map` 引用的子机器不存在；
    /// - [`CompositeError::DanglingOutputMapping`]：`output_map` 引用的子机器不存在；
    /// - [`CompositeError::DuplicatePortMapping`]：同一外部端口名同时出现在
    ///   `input_map` 和 `output_map` 中（一个端口不能既是输入又是输出）。
    ///
    /// # 不检查
    ///
    /// - 子端口名存在性（需要 `PortSchema`）；
    /// - 子拓扑内部的链接正确性（由 `DeploySpec::validate_deep` 负责）；
    /// - 复合自引用（由 `expand_composites` 的深度上限保护）。
    pub fn validate(&self) -> Result<(), CompositeError> {
        let sub_machine_names: crate::compat::HashSet<&str> =
            self.spec.machines.iter().map(|m| m.name.as_ref()).collect();

        // 1. input_map 引用的子机器必须存在。
        for (ext_port, (sub_m, _sub_p)) in &self.input_map {
            if !sub_machine_names.contains(sub_m.as_str()) {
                return Err(CompositeError::DanglingInputMapping {
                    ext_port: ext_port.clone(),
                    sub_machine: sub_m.clone(),
                });
            }
        }

        // 2. output_map 引用的子机器必须存在。
        for (ext_port, (sub_m, _sub_p)) in &self.output_map {
            if !sub_machine_names.contains(sub_m.as_str()) {
                return Err(CompositeError::DanglingOutputMapping {
                    ext_port: ext_port.clone(),
                    sub_machine: sub_m.clone(),
                });
            }
        }

        // 3. 同一外部端口名不能同时出现在 input_map 和 output_map 中。
        for ext_port in self.input_map.keys() {
            if self.output_map.contains_key(ext_port) {
                return Err(CompositeError::DuplicatePortMapping {
                    ext_port: ext_port.clone(),
                });
            }
        }

        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 2: expand_composites — 纯数据变换
// ════════════════════════════════════════════════════════════════════════════

/// 递归展开复合 Machine——把所有 `machine_type` 匹配已注册复合的实例
/// 替换为子拓扑（名字空间化），并重定向外部链接。
///
/// 这是**纯数据变换**：`DeploySpec → DeploySpec`。不依赖任何执行原语。
///
/// 循环展开直到无复合残留（处理任意深度嵌套）。返回 `Err` 当嵌套深度
/// 超过 64（几乎肯定是复合自引用导致的配置错误）。
///
/// # 参数
///
/// - `machines`：待展开的机器列表（通常来自 `DeploySpec::machines`）；
/// - `links`：待展开的链接列表（通常来自 `DeploySpec::links`）；
/// - `composites`：`machine_type → CompositeSpec` 注册表。
///
/// # 返回
///
/// - `Ok((machines, links))`：展开后的扁平拓扑，无复合实例残留；
/// - `Err(CompositeError::TooDeep)`：嵌套深度超过 64，几乎肯定是复合
///   自引用（子拓扑中包含自身类型的实例）。
///
/// # 算法
///
/// 每轮迭代：
/// 1. 扫描 `machines`，命中复合类型的实例展开为子机器（名字空间化）；
/// 2. 扫描 `links`，命中复合实例端点的链接按端口映射重定向；
/// 3. 若本轮无展开发生，返回当前结果；否则继续下一轮。
///
/// 复杂度：每轮 O(N+M)，N=机器数，M=链接数。最坏情况 O(D·(N+M))，D=深度。
pub fn expand_composites(
    mut machines: Vec<MachineInstance>,
    mut links: Vec<LinkSpec>,
    composites: &BTreeMap<String, CompositeSpec>,
) -> Result<(Vec<MachineInstance>, Vec<LinkSpec>), CompositeError> {
    // 安全阀：防止恶意无限递归（复合自引用导致无限展开）。
    // 正常嵌套深度 < 10；超过 64 几乎肯定是配置错误。
    const MAX_DEPTH: usize = 64;
    for _depth in 0..MAX_DEPTH {
        let mut next_machines: Vec<MachineInstance> = Vec::new();
        let mut next_links: Vec<LinkSpec> = Vec::new();
        // 本轮展开的复合实例名 → (input_map, output_map) 快照。
        let mut port_maps: BTreeMap<
            String,
            (&BTreeMap<String, (String, String)>, &BTreeMap<String, (String, String)>),
        > = BTreeMap::new();
        let mut found_composite = false;

        // ── 展开机器 ──
        for m in &machines {
            if let Some(comp) = composites.get(m.machine_type.as_ref()) {
                found_composite = true;
                let prefix = m.name.as_ref();
                port_maps.insert(prefix.to_string(), (&comp.input_map, &comp.output_map));

                for sub_m in &comp.spec.machines {
                    let mut expanded = sub_m.clone();
                    expanded.name = Cow::Owned(format!("{}.{}", prefix, sub_m.name));
                    next_machines.push(expanded);
                }
                // 子拓扑的链接——名字空间化两端。
                for sub_l in &comp.spec.links {
                    next_links.push(LinkSpec {
                        out: (
                            Cow::Owned(format!("{}.{}", prefix, sub_l.out.0)),
                            sub_l.out.1.clone(),
                        ),
                        into: (
                            Cow::Owned(format!("{}.{}", prefix, sub_l.into.0)),
                            sub_l.into.1.clone(),
                        ),
                        kind: sub_l.kind.clone(),
                    });
                }
            } else {
                next_machines.push(m.clone());
            }
        }

        // ── 重定向外部链接 ──
        for l in &links {
            let src_machine = l.out.0.as_ref();
            let src_port = l.out.1.as_ref();
            let dst_machine = l.into.0.as_ref();
            let dst_port = l.into.1.as_ref();

            // 源端是复合实例 → 按 output_map 重定向。
            let new_out = if let Some((_, output_map)) = port_maps.get(src_machine) {
                if let Some((sub_m, sub_p)) = output_map.get(src_port) {
                    (
                        Cow::Owned(format!("{}.{}", src_machine, sub_m)),
                        Cow::Owned(sub_p.clone()),
                    )
                } else {
                    (l.out.0.clone(), l.out.1.clone())
                }
            } else {
                (l.out.0.clone(), l.out.1.clone())
            };

            // 目标端是复合实例 → 按 input_map 重定向。
            let new_into = if let Some((input_map, _)) = port_maps.get(dst_machine) {
                if let Some((sub_m, sub_p)) = input_map.get(dst_port) {
                    (
                        Cow::Owned(format!("{}.{}", dst_machine, sub_m)),
                        Cow::Owned(sub_p.clone()),
                    )
                } else {
                    (l.into.0.clone(), l.into.1.clone())
                }
            } else {
                (l.into.0.clone(), l.into.1.clone())
            };

            next_links.push(LinkSpec {
                out: new_out,
                into: new_into,
                kind: l.kind.clone(),
            });
        }

        machines = next_machines;
        links = next_links;

        if !found_composite {
            // 所有复合已展开完毕——正常退出。
            return Ok((machines, links));
        }
        // 仍含复合实例但已用尽深度预算——配置错误（很可能复合自引用）。
        // 循环结束后落入下面的 Err。
    }

    Err(CompositeError::TooDeep {
        depth: MAX_DEPTH,
        hint: "composite machine_type may be self-referential (its sub-topology \
               contains an instance of itself). Check composite definitions for \
               cycles."
            .into(),
    })
}

// ════════════════════════════════════════════════════════════════════════════
// Section 3: 错误类型
// ════════════════════════════════════════════════════════════════════════════

/// 复合 Machine 定义或展开过程中的错误。
///
/// 这是 core 层错误——`CompositeSpec::validate` 和 `expand_composites`
/// 都返回这个类型。runtime 的 `RuntimeError::CompositeTooDeep` 是它的
/// 执行层镜像（runtime 在物化时把 `CompositeError` 转为 `RuntimeError`）。
#[derive(Debug)]
pub enum CompositeError {
    /// `input_map` 引用的子机器在 `spec.machines` 中不存在。
    DanglingInputMapping {
        /// 外部输入端口名。
        ext_port: String,
        /// 引用的子机器名（不存在）。
        sub_machine: String,
    },
    /// `output_map` 引用的子机器在 `spec.machines` 中不存在。
    DanglingOutputMapping {
        /// 外部输出端口名。
        ext_port: String,
        /// 引用的子机器名（不存在）。
        sub_machine: String,
    },
    /// 同一外部端口名同时出现在 `input_map` 和 `output_map` 中。
    ///
    /// 一个端口不能既是输入又是输出——这是方向冲突。
    DuplicatePortMapping {
        /// 冲突的外部端口名。
        ext_port: String,
    },
    /// 复合 Machine 嵌套深度超过上限（可能为复合自引用导致无限展开）。
    TooDeep {
        /// 达到的深度上限。
        depth: usize,
        /// 诊断提示。
        hint: String,
    },
}

impl core::fmt::Display for CompositeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DanglingInputMapping { ext_port, sub_machine } => write!(
                f,
                "composite input_map references non-existent sub-machine: \
                 ext_port `{ext_port}` → `{sub_machine}` (not in spec.machines)"
            ),
            Self::DanglingOutputMapping { ext_port, sub_machine } => write!(
                f,
                "composite output_map references non-existent sub-machine: \
                 ext_port `{ext_port}` → `{sub_machine}` (not in spec.machines)"
            ),
            Self::DuplicatePortMapping { ext_port } => write!(
                f,
                "external port `{ext_port}` appears in both input_map and output_map \
                 (a port cannot be both input and output)"
            ),
            Self::TooDeep { depth, hint } => write!(
                f,
                "composite expansion exceeded depth {depth}: {hint}"
            ),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CompositeError {}

// ════════════════════════════════════════════════════════════════════════════
// Section 4: Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{DeploySpec, MachineInstance};
    use crate::link::{LinkKind, LinkSpec};
    use crate::resource::MachinePhysicalSpec;

    // ── 测试辅助 ───────────────────────────────────────────────────────

    fn machine(name: &'static str) -> MachineInstance {
        MachineInstance::new(name, "test", MachinePhysicalSpec::default())
    }

    fn inline(a: &'static str, pa: &'static str, b: &'static str, pb: &'static str) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::Inline)
    }

    /// 构造一个简单的复合：子拓扑 `inner → inner2`，外部端口 `in`/`out`。
    fn simple_composite() -> CompositeSpec {
        let spec = DeploySpec::new()
            .with_machine(machine("inner"))
            .with_machine(machine("inner2"))
            .with_link(inline("inner", "y", "inner2", "x"));
        CompositeSpec::new(spec)
            .with_input("in", "inner", "x")
            .with_output("out", "inner2", "y")
    }

    // ══════════════════════════════════════════════════════════════════
    // validate() — 端口映射完整性
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_ok_simple_composite() {
        let comp = simple_composite();
        assert!(comp.validate().is_ok(), "simple composite should validate");
    }

    #[test]
    fn validate_ok_empty_port_maps() {
        // 空端口映射也合法——复合可能只有内部链接，无外部端口。
        let spec = DeploySpec::new().with_machine(machine("inner"));
        let comp = CompositeSpec::new(spec);
        assert!(comp.validate().is_ok());
    }

    #[test]
    fn validate_rejects_dangling_input_mapping() {
        // input_map 引用的子机器 "nonexistent" 不在 spec.machines 中。
        let spec = DeploySpec::new().with_machine(machine("inner"));
        let comp = CompositeSpec::new(spec).with_input("in", "nonexistent", "x");
        let err = comp.validate().unwrap_err();
        match err {
            CompositeError::DanglingInputMapping { ext_port, sub_machine } => {
                assert_eq!(ext_port, "in");
                assert_eq!(sub_machine, "nonexistent");
            }
            other => panic!("expected DanglingInputMapping, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_dangling_output_mapping() {
        // output_map 引用的子机器 "ghost" 不在 spec.machines 中。
        let spec = DeploySpec::new().with_machine(machine("inner"));
        let comp = CompositeSpec::new(spec).with_output("out", "ghost", "y");
        let err = comp.validate().unwrap_err();
        match err {
            CompositeError::DanglingOutputMapping { ext_port, sub_machine } => {
                assert_eq!(ext_port, "out");
                assert_eq!(sub_machine, "ghost");
            }
            other => panic!("expected DanglingOutputMapping, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_duplicate_port_mapping() {
        // 同一外部端口 "x" 同时出现在 input_map 和 output_map 中。
        let spec = DeploySpec::new()
            .with_machine(machine("inner"))
            .with_machine(machine("inner2"));
        let comp = CompositeSpec::new(spec)
            .with_input("x", "inner", "p1")
            .with_output("x", "inner2", "p2");
        let err = comp.validate().unwrap_err();
        match err {
            CompositeError::DuplicatePortMapping { ext_port } => {
                assert_eq!(ext_port, "x");
            }
            other => panic!("expected DuplicatePortMapping, got {other:?}"),
        }
    }

    #[test]
    fn validate_ok_multiple_inputs_outputs() {
        // 多输入多输出——全部引用存在的子机器。
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"));
        let comp = CompositeSpec::new(spec)
            .with_input("in1", "a", "x")
            .with_input("in2", "b", "x")
            .with_output("out1", "c", "y")
            .with_output("out2", "c", "z");
        assert!(comp.validate().is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // expand_composites() — 基本展开
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn expand_no_composites_returns_unchanged() {
        // 无复合实例——返回原始机器/链接（克隆）。
        let machines = vec![machine("a"), machine("b")];
        let links = vec![inline("a", "y", "b", "x")];
        let composites = BTreeMap::new();
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");
        assert_eq!(out_m.len(), 2);
        assert_eq!(out_l.len(), 1);
        assert_eq!(out_m[0].name.as_ref(), "a");
        assert_eq!(out_m[1].name.as_ref(), "b");
    }

    #[test]
    fn expand_single_composite_replaces_instance() {
        // 一个复合实例 "root" → 展开为 root.inner + root.inner2。
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![MachineInstance::new("root", "comp", MachinePhysicalSpec::default())];
        let links = vec![];
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        assert_eq!(out_m.len(), 2, "composite expands to 2 sub-machines");
        assert_eq!(out_m[0].name.as_ref(), "root.inner");
        assert_eq!(out_m[1].name.as_ref(), "root.inner2");
        assert_eq!(out_l.len(), 1, "sub-topology has 1 internal link");
        assert_eq!(out_l[0].out.0.as_ref(), "root.inner");
        assert_eq!(out_l[0].into.0.as_ref(), "root.inner2");
    }

    #[test]
    fn expand_redirects_external_input_link() {
        // 外部链接 (ext, y) → (root, in) 命中 input_map → 重定向到 (root.inner, x)。
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("root", "comp", MachinePhysicalSpec::default()),
            machine("ext"),
        ];
        let links = vec![inline("ext", "y", "root", "in")];
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        assert_eq!(out_m.len(), 3, "ext + 2 sub-machines");
        // 外部链接应被重定向到 root.inner.x
        let redirected = out_l
            .iter()
            .find(|l| l.out.0.as_ref() == "ext" && l.out.1.as_ref() == "y")
            .expect("external link should exist");
        assert_eq!(redirected.into.0.as_ref(), "root.inner");
        assert_eq!(redirected.into.1.as_ref(), "x");
    }

    #[test]
    fn expand_redirects_external_output_link() {
        // 外部链接 (root, out) → (ext, x) 命中 output_map → 重定向到 (root.inner2, y)。
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("root", "comp", MachinePhysicalSpec::default()),
            machine("ext"),
        ];
        let links = vec![inline("root", "out", "ext", "x")];
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        assert_eq!(out_m.len(), 3);
        let redirected = out_l
            .iter()
            .find(|l| l.into.0.as_ref() == "ext" && l.into.1.as_ref() == "x")
            .expect("external link should exist");
        assert_eq!(redirected.out.0.as_ref(), "root.inner2");
        assert_eq!(redirected.out.1.as_ref(), "y");
    }

    #[test]
    fn expand_unmapped_external_port_passes_through() {
        // 外部端口 "unknown" 不在 input_map/output_map 中 → 链接保持原样
        // （指向 root.unknown，后续 validate_deep 会报 DanglingRef）。
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("root", "comp", MachinePhysicalSpec::default()),
            machine("ext"),
        ];
        let links = vec![inline("ext", "y", "root", "unknown")];
        let (_out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        // 未映射的端口保持原样——后续 validate_deep 会捕获。
        let unredirected = out_l
            .iter()
            .find(|l| l.out.0.as_ref() == "ext")
            .expect("external link should exist");
        assert_eq!(unredirected.into.0.as_ref(), "root");
        assert_eq!(unredirected.into.1.as_ref(), "unknown");
    }

    // ══════════════════════════════════════════════════════════════════
    // expand_composites() — 嵌套与深度
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn expand_nested_composite_two_levels() {
        // 外层复合 "outer" 包含一个内层复合 "inner" 实例。
        // 展开后：root.outer.inner.sub1, root.outer.inner.sub2
        let inner_spec = DeploySpec::new()
            .with_machine(machine("sub1"))
            .with_machine(machine("sub2"))
            .with_link(inline("sub1", "y", "sub2", "x"));
        let inner_comp = CompositeSpec::new(inner_spec)
            .with_input("in", "sub1", "x")
            .with_output("out", "sub2", "y");

        let outer_spec = DeploySpec::new()
            .with_machine(MachineInstance::new("inner_inst", "inner_type", MachinePhysicalSpec::default()));
        let outer_comp = CompositeSpec::new(outer_spec)
            .with_input("in", "inner_inst", "in")
            .with_output("out", "inner_inst", "out");

        let mut composites = BTreeMap::new();
        composites.insert("inner_type".to_string(), inner_comp);
        composites.insert("outer_type".to_string(), outer_comp);

        let machines = vec![MachineInstance::new("root", "outer_type", MachinePhysicalSpec::default())];
        let (out_m, _out_l) = expand_composites(machines, vec![], &composites).expect("expand");

        // 两轮展开：outer → inner_inst.inner_type → sub1/sub2
        assert_eq!(out_m.len(), 2);
        assert_eq!(out_m[0].name.as_ref(), "root.inner_inst.sub1");
        assert_eq!(out_m[1].name.as_ref(), "root.inner_inst.sub2");
    }

    #[test]
    fn expand_self_referential_composite_reports_too_deep() {
        // 复合 "loop" 的子拓扑包含自身类型的实例 → 无限展开 → TooDeep。
        let loop_spec = DeploySpec::new().with_machine(MachineInstance::new(
            "inner",
            "loop",
            MachinePhysicalSpec::default(),
        ));
        let loop_comp = CompositeSpec::new(loop_spec)
            .with_input("in", "inner", "in")
            .with_output("out", "inner", "out");

        let mut composites = BTreeMap::new();
        composites.insert("loop".to_string(), loop_comp);

        let machines = vec![MachineInstance::new("root", "loop", MachinePhysicalSpec::default())];
        let err = expand_composites(machines, vec![], &composites).unwrap_err();
        match err {
            CompositeError::TooDeep { depth, .. } => {
                assert_eq!(depth, 64, "MAX_DEPTH = 64");
            }
            other => panic!("expected TooDeep, got {other:?}"),
        }
    }

    #[test]
    fn expand_multiple_composites_in_parallel() {
        // 两个独立的复合实例同时展开——名字空间互不冲突。
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("a", "comp", MachinePhysicalSpec::default()),
            MachineInstance::new("b", "comp", MachinePhysicalSpec::default()),
        ];
        let (out_m, out_l) = expand_composites(machines, vec![], &composites).expect("expand");

        assert_eq!(out_m.len(), 4, "2 composites × 2 sub-machines each");
        // 两个复合实例的子机器名字空间不同。
        let names: Vec<&str> = out_m.iter().map(|m| m.name.as_ref()).collect();
        assert!(names.contains(&"a.inner"));
        assert!(names.contains(&"a.inner2"));
        assert!(names.contains(&"b.inner"));
        assert!(names.contains(&"b.inner2"));
        // 每个复合各 1 条内部链接。
        assert_eq!(out_l.len(), 2);
    }

    // ══════════════════════════════════════════════════════════════════
    // expand_composites() — 边界情况
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn expand_empty_machines_returns_empty() {
        let (out_m, out_l) =
            expand_composites(vec![], vec![], &BTreeMap::new()).expect("expand");
        assert!(out_m.is_empty());
        assert!(out_l.is_empty());
    }

    #[test]
    fn expand_unregistered_composite_type_passes_through() {
        // machine_type "unknown_comp" 未注册——实例原样保留。
        let machines = vec![MachineInstance::new(
            "x",
            "unknown_comp",
            MachinePhysicalSpec::default(),
        )];
        let (out_m, _out_l) =
            expand_composites(machines, vec![], &BTreeMap::new()).expect("expand");
        assert_eq!(out_m.len(), 1);
        assert_eq!(out_m[0].name.as_ref(), "x");
        assert_eq!(out_m[0].machine_type.as_ref(), "unknown_comp");
    }

    #[test]
    fn expand_preserves_machine_physical_spec() {
        // 展开后的子机器应保留原始 sub-machine 的 physical spec。
        use crate::resource::ExecutionHint;
        let mut inner_physical = MachinePhysicalSpec::default();
        inner_physical.execution = ExecutionHint::CpuBound;
        let inner = MachineInstance::new("inner", "test", inner_physical);
        let spec = DeploySpec::new().with_machine(inner);
        let comp = CompositeSpec::new(spec);

        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), comp);

        let machines = vec![MachineInstance::new("root", "comp", MachinePhysicalSpec::default())];
        let (out_m, _) = expand_composites(machines, vec![], &composites).expect("expand");

        assert_eq!(out_m.len(), 1);
        assert!(matches!(
            out_m[0].physical.execution,
            ExecutionHint::CpuBound
        ));
    }

    #[test]
    fn expand_preserves_link_kind() {
        // 展开后的链接应保留原始 LinkKind（包括 BoundedBuf 的参数）。
        use crate::link::{ReadPolicy, WritePolicy};
        let bounded = LinkSpec::new(
            ("inner", "y"),
            ("inner2", "x"),
            LinkKind::BoundedBuf {
                capacity: 42,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::NonBlocking,
            },
        );
        let spec = DeploySpec::new()
            .with_machine(machine("inner"))
            .with_machine(machine("inner2"))
            .with_link(bounded);
        let comp = CompositeSpec::new(spec);

        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), comp);

        let machines = vec![MachineInstance::new("root", "comp", MachinePhysicalSpec::default())];
        let (out_m, out_l) = expand_composites(machines, vec![], &composites).expect("expand");

        assert_eq!(out_m.len(), 2);
        assert_eq!(out_l.len(), 1);
        match &out_l[0].kind {
            LinkKind::BoundedBuf {
                capacity,
                write_policy,
                read_policy,
            } => {
                assert_eq!(*capacity, 42);
                assert_eq!(*write_policy, WritePolicy::Dropping);
                assert_eq!(*read_policy, ReadPolicy::NonBlocking);
            }
            other => panic!("expected BoundedBuf, got {other:?}"),
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // CompositeSpec builder API
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn builder_with_input_chains() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"));
        let comp = CompositeSpec::new(spec)
            .with_input("in1", "a", "x")
            .with_input("in2", "b", "x");
        assert_eq!(comp.input_map.len(), 2);
        assert!(comp.input_map.contains_key("in1"));
        assert!(comp.input_map.contains_key("in2"));
    }

    #[test]
    fn builder_with_output_chains() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"));
        let comp = CompositeSpec::new(spec)
            .with_output("out1", "a", "y")
            .with_output("out2", "b", "y");
        assert_eq!(comp.output_map.len(), 2);
        assert!(comp.output_map.contains_key("out1"));
        assert!(comp.output_map.contains_key("out2"));
    }

    #[test]
    fn builder_with_input_and_output() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"));
        let comp = CompositeSpec::new(spec)
            .with_input("in", "a", "x")
            .with_output("out", "b", "y");
        assert_eq!(comp.input_map.len(), 1);
        assert_eq!(comp.output_map.len(), 1);
        assert!(comp.validate().is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // 错误显示
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn error_display_dangling_input() {
        let err = CompositeError::DanglingInputMapping {
            ext_port: "in".into(),
            sub_machine: "ghost".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("input_map"));
        assert!(s.contains("in"));
        assert!(s.contains("ghost"));
    }

    #[test]
    fn error_display_dangling_output() {
        let err = CompositeError::DanglingOutputMapping {
            ext_port: "out".into(),
            sub_machine: "phantom".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("output_map"));
        assert!(s.contains("out"));
        assert!(s.contains("phantom"));
    }

    #[test]
    fn error_display_duplicate_port() {
        let err = CompositeError::DuplicatePortMapping {
            ext_port: "x".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("x"));
        assert!(s.contains("both"));
    }

    #[test]
    fn error_display_too_deep() {
        let err = CompositeError::TooDeep {
            depth: 64,
            hint: "self-referential".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("64"));
        assert!(s.contains("self-referential"));
    }
}
