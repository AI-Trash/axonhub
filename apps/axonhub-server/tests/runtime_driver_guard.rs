use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const TASK_11_RUNTIME_SCOPE_FILES: [&str; 5] = [
    "src/foundation/identity.rs",
    "src/foundation/request_context.rs",
    "src/foundation/graphql.rs",
    "src/foundation/admin_operational.rs",
    "src/foundation/openai_v1.rs",
];

const TASK_11_RUNTIME_REPOSITORY_FILES: [&str; 7] = [
    "src/foundation/repositories/mod.rs",
    "src/foundation/repositories/admin.rs",
    "src/foundation/repositories/graphql.rs",
    "src/foundation/repositories/identity.rs",
    "src/foundation/repositories/openai_v1.rs",
    "src/foundation/repositories/prompt_protection.rs",
    "src/foundation/repositories/request_context.rs",
];

#[test]
fn runtime_driver_guard_allows_test_only_usage() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let violations = scan_runtime_driver_usage(&repo_root);

    assert!(
        violations.is_empty(),
        "unexpected direct-driver usage in runtime files:\n{}",
        violations.join("\n")
    );
}

#[test]
fn seaorm_runtime_policy_guard_passes() {
    runtime_driver_guard_allows_test_only_usage();
}

#[test]
fn task_11_runtime_scope_remains_rusqlite_free() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scope_files = collect_task_11_runtime_scope_files(&repo_root);
    let violations = scan_files_for_forbidden_token(&scope_files, "rusqlite");

    assert!(
        violations.is_empty(),
        "Task 11 runtime scope must stay rusqlite-free:\n{}",
        violations.join("\n")
    );

    let covered_files = scope_files
        .iter()
        .map(|path| {
            path.strip_prefix(&repo_root)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    assert!(
        TASK_11_RUNTIME_SCOPE_FILES
            .iter()
            .all(|relative| covered_files.contains(*relative)),
        "Task 11 runtime scope did not include the required production files: {:?}",
        TASK_11_RUNTIME_SCOPE_FILES
    );
    assert!(
        TASK_11_RUNTIME_REPOSITORY_FILES
            .iter()
            .all(|relative| covered_files.contains(*relative)),
        "Task 11 runtime scope did not include the required repository files: {:?}",
        TASK_11_RUNTIME_REPOSITORY_FILES
    );
}

#[test]
fn runtime_driver_guard_rejects_runtime_imports() {
    let root = unique_temp_dir("runtime-driver-guard-violation");
    let src_dir = root.join("src");
    let app_dir = src_dir.join("app");
    let foundation_dir = src_dir.join("foundation");

    fs::create_dir_all(&app_dir).expect("create app dir");
    fs::create_dir_all(&foundation_dir).expect("create foundation dir");

    fs::write(
        app_dir.join("mod.rs"),
        "pub(crate) mod runtime;\n#[cfg(test)]\npub(crate) mod tests;\n",
    )
    .expect("write app mod");
    fs::write(
        foundation_dir.join("mod.rs"),
        "pub(crate) mod runtime;\n#[cfg(test)]\npub(crate) mod sqlite_test_support;\n",
    )
    .expect("write foundation mod");
    fs::write(
        app_dir.join("runtime.rs"),
        "use postgres::Client;\n\nfn build() -> Client {\n    unimplemented!()\n}\n",
    )
    .expect("write violating runtime file");
    fs::write(app_dir.join("tests.rs"), "use rusqlite::Connection;\n")
        .expect("write allowed test file");
    fs::write(
        foundation_dir.join("sqlite_test_support.rs"),
        "use rusqlite::Connection;\n",
    )
    .expect("write allowed foundation helper");

    let violations = scan_runtime_driver_usage(&root);

    assert_eq!(
        violations.len(),
        1,
        "expected exactly one runtime violation"
    );
    assert!(
        violations[0].contains("src/app/runtime.rs")
            || violations[0].contains("src\\app\\runtime.rs"),
        "unexpected violation: {}",
        violations[0]
    );
    assert!(
        violations[0].contains("postgres"),
        "unexpected violation: {}",
        violations[0]
    );
}

#[test]
fn runtime_driver_guard_allows_inline_cfg_test_support_module() {
    let root = unique_temp_dir("runtime-driver-guard-inline-test-support");
    let src_dir = root.join("src");
    let foundation_dir = src_dir.join("foundation");

    fs::create_dir_all(&foundation_dir).expect("create foundation dir");

    fs::write(foundation_dir.join("mod.rs"), "pub(crate) mod system;\n")
        .expect("write foundation mod");
    fs::write(
        foundation_dir.join("system.rs"),
        "fn runtime_ok() {}\n\n#[cfg(test)]\npub(crate) mod sqlite_test_support {\n    use rusqlite::Connection;\n\n    pub(crate) fn open() -> Connection {\n        unimplemented!()\n    }\n}\n",
    )
    .expect("write inline test-support module");

    let violations = scan_runtime_driver_usage(&root);

    assert!(
        violations.is_empty(),
        "inline cfg(test) test-support module should be ignored by runtime guard:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_driver_guard_rejects_runtime_driver_paths() {
    let root = unique_temp_dir("runtime-driver-guard-path-violation");
    let src_dir = root.join("src");
    let app_dir = src_dir.join("app");
    let foundation_dir = src_dir.join("foundation");

    fs::create_dir_all(&app_dir).expect("create app dir");
    fs::create_dir_all(&foundation_dir).expect("create foundation dir");

    fs::write(app_dir.join("mod.rs"), "pub(crate) mod runtime;\n").expect("write app mod");
    fs::write(foundation_dir.join("mod.rs"), "pub(crate) mod runtime;\n")
        .expect("write foundation mod");
    fs::write(
        app_dir.join("runtime.rs"),
        "fn bind(value: &(dyn postgres::types::ToSql + Sync)) {\n    let _ = value;\n}\n",
    )
    .expect("write violating runtime path file");
    fs::write(
        foundation_dir.join("runtime.rs"),
        "fn ok() {\n    let _ = 1;\n}\n",
    )
    .expect("write non-violating foundation runtime file");

    let violations = scan_runtime_driver_usage(&root);

    assert_eq!(
        violations.len(),
        1,
        "expected exactly one runtime path-fragment violation"
    );
    assert!(
        violations[0].contains("src/app/runtime.rs")
            || violations[0].contains("src\\app\\runtime.rs"),
        "unexpected violation: {}",
        violations[0]
    );
    assert!(
        violations[0].contains("postgres"),
        "unexpected violation: {}",
        violations[0]
    );
}

fn scan_runtime_driver_usage(repo_root: &Path) -> Vec<String> {
    let src_root = repo_root.join("src");
    let mut test_only_files = BTreeSet::new();

    for module_root in [src_root.join("app"), src_root.join("foundation")] {
        test_only_files.extend(collect_test_only_modules(&module_root));
    }

    let mut violations = Vec::new();
    collect_rust_files(&src_root, &mut |path| {
        if is_explicit_test_file(path) || is_test_only_module_file(path, &test_only_files) {
            return;
        }

        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };

        let inline_test_ranges = inline_cfg_test_module_ranges(&contents);

        for (index, line) in contents.lines().enumerate() {
            if line_is_in_ranges(index + 1, &inline_test_ranges) {
                continue;
            }

            if let Some(driver) = direct_driver_reference(line) {
                violations.push(format!(
                    "{}:{}: runtime file references forbidden `{}`",
                    path.display(),
                    index + 1,
                    driver
                ));
            }
        }
    });

    violations
}

fn collect_task_11_runtime_scope_files(repo_root: &Path) -> Vec<PathBuf> {
    let mut files = TASK_11_RUNTIME_SCOPE_FILES
        .iter()
        .map(|relative| repo_root.join(relative))
        .collect::<Vec<_>>();

    files.extend(
        TASK_11_RUNTIME_REPOSITORY_FILES
            .iter()
            .map(|relative| repo_root.join(relative)),
    );

    files.sort();
    files.dedup();
    files
}

fn scan_files_for_forbidden_token(paths: &[PathBuf], forbidden: &str) -> Vec<String> {
    let mut violations = Vec::new();

    for path in paths {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };

        let inline_test_ranges = inline_cfg_test_module_ranges(&contents);

        for (index, line) in contents.lines().enumerate() {
            let trimmed = line.trim_start();
            if line_is_in_ranges(index + 1, &inline_test_ranges)
                || trimmed.starts_with("//")
                || !trimmed.contains(forbidden)
            {
                continue;
            }

            violations.push(format!(
                "{}:{}: runtime file references forbidden `{}`",
                path.display(),
                index + 1,
                forbidden
            ));
        }
    }

    violations
}

fn collect_test_only_modules(module_root: &Path) -> BTreeSet<String> {
    let mut modules = BTreeSet::new();
    let mod_rs = module_root.join("mod.rs");
    let Ok(contents) = fs::read_to_string(&mod_rs) else {
        return modules;
    };

    let mut pending_cfg_test = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
            continue;
        }

        if pending_cfg_test {
            if let Some(module_name) = parse_module_decl(trimmed) {
                modules.insert(module_name);
            }
            pending_cfg_test = false;
        }
    }

    modules
}

fn parse_module_decl(line: &str) -> Option<String> {
    let line = line.trim_start();
    let line = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    let line = line.strip_prefix("mod ")?;
    let module_name = line.split(';').next()?.trim();
    (!module_name.is_empty()).then(|| module_name.to_owned())
}

fn collect_rust_files<F>(root: &Path, visit: &mut F)
where
    F: FnMut(&Path),
{
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, visit);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            visit(&path);
        }
    }
}

fn is_explicit_test_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("tests.rs")
    ) || path
        .components()
        .any(|component| component.as_os_str() == "tests")
}

fn is_test_only_module_file(path: &Path, test_only_modules: &BTreeSet<String>) -> bool {
    let stem = path.file_stem().and_then(|value| value.to_str());
    if let Some(stem) = stem {
        if test_only_modules.contains(stem) {
            return true;
        }
    }

    false
}

fn direct_driver_reference(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    for driver in ["rusqlite", "postgres"] {
        let import_prefix = format!("use {driver}");
        let pub_import_prefix = format!("pub use {driver}");
        let path_fragment = format!("{driver}::");

        if trimmed.starts_with(&import_prefix)
            || trimmed.starts_with(&pub_import_prefix)
            || trimmed.contains(&path_fragment)
        {
            return Some(driver);
        }
    }

    None
}

fn inline_cfg_test_module_ranges(contents: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut ranges = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        if !lines[index].trim_start().starts_with("#[cfg(test)]") {
            index += 1;
            continue;
        }

        let mut module_line = index + 1;
        while module_line < lines.len() && lines[module_line].trim().is_empty() {
            module_line += 1;
        }

        if module_line >= lines.len() {
            break;
        }

        if let Some(module_start) = inline_test_module_start(lines[module_line]) {
            if let Some(module_end) = matching_brace_end(&lines, module_line, module_start) {
                ranges.push((index + 1, module_end));
            }
        }

        index = module_line + 1;
    }

    ranges
}

fn inline_test_module_start(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let trimmed = trimmed
        .strip_prefix("pub(crate) ")
        .or_else(|| trimmed.strip_prefix("pub(super) "))
        .or_else(|| trimmed.strip_prefix("pub "))
        .unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix("mod ")?;
    let brace_index = trimmed.find('{')?;
    let module_name = trimmed[..brace_index].trim();
    if module_name.is_empty() {
        return None;
    }
    Some(brace_index)
}

fn matching_brace_end(
    lines: &[&str],
    start_line: usize,
    start_brace_index: usize,
) -> Option<usize> {
    let mut depth = 0isize;
    let mut started = false;

    for (line_index, line) in lines.iter().enumerate().skip(start_line) {
        let chars = if line_index == start_line {
            line.chars().skip(start_brace_index).collect::<Vec<_>>()
        } else {
            line.chars().collect::<Vec<_>>()
        };

        for ch in chars {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => {
                    depth -= 1;
                    if started && depth == 0 {
                        return Some(line_index + 1);
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn line_is_in_ranges(line_number: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line_number >= *start && line_number <= *end)
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("axonhub-{name}-{unique}"))
}
