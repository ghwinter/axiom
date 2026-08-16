//! 源码维度 lint：扫描 crate 源码，检查 `Machine::name()` 唯一性。
//!
//! 这是"源码维度 lint"——`lint.rs` 检查 `DeploySpec` 蓝图实例，本测试
//! 检查**源码中的声明一致性**：机器名是拓扑引用的标识（`LinkSpec` 端点、
//! `machine_type`），两个 `Machine` 实现返回相同的 `name()` 会破坏蓝图
//! 引用。此测试在测试期（std 环境）扫描 `src/` 全部 `.rs`，只收集
//! `impl Machine for ...` 块内的 `name()`，断言名字唯一。
//!
//! 说明：`Func` 组合子（如 `FuncScratchPipeline`）的多个 `impl` 共享
//! 一个家族名是合法的（"pipeline" 2 步/3 步），故**不**纳入扫描范围。

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 从一行源码提取 `fn name() -> &'static str { "..." }` 的字面量。
fn machine_name_in_line(line: &str) -> Option<String> {
    let needle = "fn name() -> &'static str";
    let pos = line.find(needle)?;
    let rest = &line[pos + needle.len()..];
    let q1 = rest.find('"')?;
    let after = &rest[q1 + 1..];
    let q2 = after.find('"')?;
    Some(after[..q2].to_string())
}

/// 扫描单文件，收集 `impl Machine for` 块内的 `name()` 字面量。
fn scan_file(content: &str, path: &Path, names: &mut HashMap<String, String>) {
    let mut in_machine_impl = false;
    let mut depth: i32 = 0;

    for (i, line) in content.lines().enumerate() {
        // 1. 进入 Machine impl 块（" Machine for" 排除 HybridMachine）。
        if !in_machine_impl && line.contains(" Machine for") {
            in_machine_impl = true;
            depth = 0;
        }

        // 2. 在 Machine impl 块内收集 name()。
        if in_machine_impl {
            if let Some(name) = machine_name_in_line(line) {
                let loc = format!("{}:{}", path.display(), i + 1);
                if let Some(prev) = names.insert(name.clone(), loc.clone()) {
                    panic!("duplicate Machine name `{name}` at {prev} and {loc}");
                }
            }
        }

        // 3. 更新大括号深度。
        depth += line.chars().filter(|&c| c == '{').count() as i32;
        depth -= line.chars().filter(|&c| c == '}').count() as i32;

        // 4. 深度归零即退出 Machine impl 块。
        if in_machine_impl && depth <= 0 {
            in_machine_impl = false;
        }
    }
}

/// 递归扫描目录，收集所有 Machine `name()` 到 `names`（值 = 位置）。
fn scan_dir(dir: &Path, names: &mut HashMap<String, String>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, names);
        } else if path.extension().map_or(false, |e| e == "rs") {
            let content = fs::read_to_string(&path).expect("read file");
            scan_file(&content, &path, names);
        }
    }
}

#[test]
fn machine_names_are_unique() {
    let mut names: HashMap<String, String> = HashMap::new();
    let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    scan_dir(src, &mut names);
    // 下界守卫：至少应扫描到若干机器名，防止扫描逻辑失效导致空扫描误过。
    assert!(
        names.len() >= 8,
        "expected >= 8 machine names, got {}",
        names.len()
    );
}
