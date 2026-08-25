//! 文档代码块编译漂移门（C11；dsh type-equiv 先例的极简版）。
//!
//! 抽取 `docs/{en-us,zh-cn}` 的 ```rust 围栏块（跳过 ```rust,ignore），写入一次性
//! crate 的 src/bin/ 并 `cargo check`。任何块失败 ⟹ 文档陈述与当前 API 脱钩。
//!
//! **运行方式**：默认 `#[ignore]`（内嵌 cargo check，避免拖慢常规测试）；CI 显式执行
//! `cargo test --test docgate -- --ignored --nocapture`。
//! **忽略清单**：`tmp/docgate-ignore.txt`，每行一个块 ID（如 `docs/zh-cn/core.md#3`）。

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("runtime 位于仓库根下")
        .to_path_buf()
}

/// 手写围栏扫描：返回 (block_id, 起始行, code)；跳过 `rust,ignore` 与空块。
fn extract(md: &Path) -> Vec<(String, usize, String)> {
    let text = std::fs::read_to_string(md).unwrap_or_default();
    let mut out = Vec::new();
    let mut inside = false;
    let mut ignore = false;
    let mut start = 0usize;
    let mut idx = 0usize;
    let mut buf = String::new();
    for (n, line) in text.lines().enumerate() {
        if !inside {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("```rust") {
                inside = true;
                ignore = rest.contains("ignore");
                start = n + 1;
                idx += 1;
                buf.clear();
            }
        } else if line.trim_start().starts_with("```") {
            let bid = format!("{}#{}", md.to_string_lossy().replace('\\', "/"), idx);
            if !ignore && !buf.trim().is_empty() {
                out.push((bid, start, buf.clone()));
            }
            inside = false;
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    out
}

fn failed_bins(root: &Path, units: &[(String, String)]) -> Vec<String> {
    let work = root.join("tmp/docgate-work");
    let _ = std::fs::remove_dir_all(&work);
    let src = work.join("src");
    std::fs::create_dir_all(&src).expect("scratch dir");
    std::fs::write(
        work.join("Cargo.toml"),
        format!(
            "[package]\nname = \"axiom-docgate\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\naxiom = {{ path = {:?} }}\n\n[workspace]\n",
            root.display()
        ),
    )
    .expect("Cargo.toml");
    for (i, (_bid, code)) in units.iter().enumerate() {
        let body = if code.contains("fn main") {
            code.clone()
        } else {
            format!("{code}\nfn main() {{}}")
        };
        std::fs::write(
            src.join(format!("g{i:03}.rs")),
            format!(
                "#![allow(dead_code, unused_variables, unused_imports, unused_mut)]\nuse axiom::*;\n{body}"
            ),
        )
        .expect("bin source");
    }
    let out = Command::new("cargo")
        .args(["check", "--quiet", "--message-format=short"])
        .env("CARGO_TARGET_DIR", work.join("target"))
        .current_dir(&work)
        .output()
        .expect("cargo check");
    let combined = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let mut failed = Vec::new();
    for (i, _) in units.iter().enumerate() {
        let name = format!("g{i:03}.rs");
        if combined.contains(&name) {
            failed.push(format!("g{i:03}"));
        }
    }
    failed
}

#[test]
#[ignore = "内嵌 cargo check；CI 以 --ignored 显式执行"]
fn doc_rust_blocks_compile_against_current_api() {
    let root = repo_root();
    let docs = root.join("docs");
    let mut units: Vec<(String, String)> = Vec::new();
    let mut meta: Vec<(String, usize)> = Vec::new();
    for lang in ["en-us", "zh-cn"] {
        for md in [
            docs.join(lang).join("foundations.md"),
            docs.join(lang).join("core.md"),
            docs.join(lang).join("unified.md"),
            docs.join(lang).join("runtime.md"),
        ] {
            if !md.exists() {
                continue;
            }
            for (bid, line, code) in extract(&md) {
                meta.push((bid.clone(), line));
                units.push((bid, code));
            }
        }
    }
    assert!(!units.is_empty(), "未抽取到任何 ```rust 块——门空转即失效");

    let failed = failed_bins(&root, &units);

    let ignore_file = root.join("tmp/docgate-ignore.txt");
    let ignored: Vec<String> = std::fs::read_to_string(&ignore_file)
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect())
        .unwrap_or_default();

    let mut hard_failures = Vec::new();
    for (i, &(ref bid, line)) in meta.iter().enumerate() {
        let bin = format!("g{i:03}");
        let status = if failed.contains(&bin) {
            let is_ignored = ignored.iter().any(|p| bid.starts_with(p.as_str()));
            if is_ignored {
                "FAIL(ignored)"
            } else {
                hard_failures.push((bid.clone(), line));
                "FAIL"
            }
        } else {
            "PASS"
        };
        println!("{status:>13}  {bid}  (line {line})");
    }

    assert!(
        hard_failures.is_empty(),
        "文档代码块与 API 脱钩 {} 处：\n{}",
        hard_failures.len(),
        hard_failures
            .iter()
            .map(|(b, l)| format!("  {b} @line {l}\n    → 修复文档，或在 tmp/docgate-ignore.txt 登记 `{b}` 并附原因注释"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
