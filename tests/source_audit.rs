//! Source-level lint: scans the crate source to verify that `Machine::name()` is unique.
//!
//! This is the "source-dimension lint" — `lint.rs` checks `DynamicTopology` blueprint
//! instances, whereas this test checks **declaration consistency in the source**: the
//! machine name is the identifier topology references use (`LinkSpec` endpoints,
//! `machine_type`), so two `Machine` implementations returning the same `name()` would
//! break those blueprint references. During testing (std environment) this test scans
//! every `.rs` file under `src/`, collecting only the `name()` inside
//! `impl Machine for ...` blocks, and asserts that names are unique.
//!
//! Note: multiple `impl`s of a `Func` combinator (e.g. `FuncScratchPipeline`) may
//! legitimately share a family name ("pipeline" 2-step / 3-step), so they are **not**
//! included in the scan.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Extract the `fn name() -> &'static str { "..." }` literal from a single source line.
fn machine_name_in_line(line: &str) -> Option<String> {
    let needle = "fn name() -> &'static str";
    let pos = line.find(needle)?;
    let rest = &line[pos + needle.len()..];
    let q1 = rest.find('"')?;
    let after = &rest[q1 + 1..];
    let q2 = after.find('"')?;
    Some(after[..q2].to_string())
}

/// Scan a single file, collecting `name()` literals inside `impl Machine for` blocks.
fn scan_file(content: &str, path: &Path, names: &mut HashMap<String, String>) {
    let mut in_machine_impl = false;
    let mut depth: i32 = 0;

    for (i, line) in content.lines().enumerate() {
        // 1. Enter a Machine impl block (" Machine for" excludes HybridMachine).
        if !in_machine_impl && line.contains(" Machine for") {
            in_machine_impl = true;
            depth = 0;
        }

        // 2. Collect name() inside the Machine impl block.
        if in_machine_impl {
            if let Some(name) = machine_name_in_line(line) {
                let loc = format!("{}:{}", path.display(), i + 1);
                if let Some(prev) = names.insert(name.clone(), loc.clone()) {
                    panic!("duplicate Machine name `{name}` at {prev} and {loc}");
                }
            }
        }

        // 3. Update brace depth.
        depth += line.chars().filter(|&c| c == '{').count() as i32;
        depth -= line.chars().filter(|&c| c == '}').count() as i32;

        // 4. Leave the Machine impl block once depth returns to zero.
        if in_machine_impl && depth <= 0 {
            in_machine_impl = false;
        }
    }
}

/// Recursively scan a directory, collecting all Machine `name()`s into `names` (value = location).
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
    // Lower-bound guard: at least some machine names must be scanned, so a broken
    // scan cannot silently pass with zero results.
    assert!(
        names.len() >= 8,
        "expected >= 8 machine names, got {}",
        names.len()
    );
}
