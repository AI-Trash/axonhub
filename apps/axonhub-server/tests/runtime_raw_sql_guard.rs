use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PACKAGE_PATH_PREFIX: &str = "apps/axonhub-server";
const FOUNDATION_RUNTIME_ROOT: &str = "src/foundation";
const RUNTIME_SCAN_ROOTS: [&str; 2] = ["src/app", FOUNDATION_RUNTIME_ROOT];
const RAW_SQL_ENTRY_PATTERNS: [&str; 2] =
    ["Statement::from_string(", "Statement::from_sql_and_values("];
const SQLITE_SUPPORT_SUFFIX: &str = "sqlite_support.rs";
const TARGET_RUNTIME_RAW_SQL_EXCEPTION_COUNT: usize = 0;
const TARGET_SQLITE_SUPPORT_SUFFIX_FILE_COUNT: usize = 0;

#[derive(Debug, Default)]
struct RawSqlScanReport {
    violations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawSqlOccurrence {
    entry_point: &'static str,
    line: usize,
    normalized_offset: usize,
    fragment: String,
}

#[derive(Debug, Default)]
struct NormalizedRuntimeSource {
    text: String,
    byte_lines: Vec<usize>,
}

#[test]
fn runtime_raw_sql_target_contract_requires_zero_exceptions() {
    assert_runtime_raw_sql_target_contract();
}

#[test]
fn runtime_raw_sql_guard_enforces_zero_exception_policy() {
    assert_runtime_raw_sql_target_contract();

    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = scan_runtime_raw_sql_usage(&package_root, PACKAGE_PATH_PREFIX);

    assert_no_runtime_raw_sql_violations(&report);
}

#[test]
fn runtime_raw_sql_count_is_zero() {
    runtime_raw_sql_guard_enforces_zero_exception_policy();
}

#[test]
fn sqlite_support_suffix_target_contract_requires_zero_files() {
    assert_sqlite_support_suffix_target_contract();
}

#[test]
fn sqlite_support_suffix_guard_tracks_remaining_files_until_zero_file_target() {
    assert_sqlite_support_suffix_target_contract();

    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let found_files = scan_sqlite_support_suffix_files(&package_root, PACKAGE_PATH_PREFIX);
    assert_no_sqlite_support_suffix_files(&found_files);
}

#[test]
fn sqlite_support_suffix_guard_rejects_reintroduced_suffix_file() {
    let repo_root = unique_temp_dir("sqlite-support-suffix-guard-violation");
    let package_root = repo_root.join(PACKAGE_PATH_PREFIX);
    let foundation_root = package_root.join(FOUNDATION_RUNTIME_ROOT);

    fs::create_dir_all(&foundation_root).expect("create foundation dir");
    fs::write(
        foundation_root.join("mod.rs"),
        "pub(crate) mod system;\n#[cfg(test)]\npub(crate) mod sqlite_test_support;\n",
    )
    .expect("write foundation mod");
    fs::write(
        foundation_root.join("future_sqlite_support.rs"),
        "#[cfg(test)]\npub(crate) use super::system::sqlite_test_support::SqliteFoundation;\n",
    )
    .expect("write forbidden suffix file");

    let found_files = scan_sqlite_support_suffix_files(&package_root, PACKAGE_PATH_PREFIX);

    assert_eq!(
        found_files,
        vec!["apps/axonhub-server/src/foundation/future_sqlite_support.rs".to_owned()],
        "suffix-file guard should report any reintroduced *sqlite_support.rs file"
    );
}

#[test]
fn runtime_raw_sql_guard_rejects_new_occurrence() {
    let repo_root = unique_temp_dir("runtime-raw-sql-guard-violation");
    let package_root = repo_root.join(PACKAGE_PATH_PREFIX);
    let foundation_root = package_root.join(FOUNDATION_RUNTIME_ROOT);
    let tests_dir = foundation_root.join("tests");

    fs::create_dir_all(&foundation_root).expect("create foundation dir");
    fs::create_dir_all(&tests_dir).expect("create foundation tests dir");

    fs::write(
        foundation_root.join("mod.rs"),
        "pub(crate) mod seaorm;\npub(crate) mod admin_operational;\npub(crate) mod runtime_violation;\n#[cfg(test)]\npub(crate) mod sqlite_test_support;\n",
    )
    .expect("write foundation mod");
    fs::write(
        foundation_root.join("admin_operational.rs"),
        "fn run_gc_cleanup_now(connection: &Db) {\n    let _ = connection.execute(\n        Statement ::\n            from_string(\n                connection.get_database_backend(),\n                \"VACUUM\".to_owned(),\n            ),\n    );\n}\n",
    )
    .expect("write violating operational raw SQL");
    fs::write(
        foundation_root.join("runtime_violation.rs"),
        "fn undocumented(connection: &Db, backend: DatabaseBackend) {\n    let _ = connection.execute(\n        Statement::from_sql_and_values(\n            backend,\n            \"DELETE FROM systems WHERE id = $1\",\n            [1_i64.into()],\n        ),\n    );\n}\n\n#[cfg(test)]\nmod tests {\n    fn allowed_only_in_cfg_test(connection: &Db, backend: DatabaseBackend) {\n        let _ = connection.execute(Statement::from_string(backend, \"SELECT 1\".to_owned()));\n    }\n}\n",
    )
    .expect("write violating runtime file");
    fs::write(
        foundation_root.join("sqlite_test_support.rs"),
        "fn allowed_test_only_module(connection: &Db, backend: DatabaseBackend) {\n    let _ = connection.execute(Statement::from_string(backend, \"SELECT 1\".to_owned()));\n}\n",
    )
    .expect("write allowed test-only module");
    fs::write(
        foundation_root.join("tests.rs"),
        "fn allowed_explicit_test_file(connection: &Db, backend: DatabaseBackend) {\n    let _ = connection.execute(Statement::from_string(backend, \"SELECT 1\".to_owned()));\n}\n",
    )
    .expect("write allowed explicit test file");
    fs::write(
        tests_dir.join("support.rs"),
        "fn allowed_tests_subdir(connection: &Db, backend: DatabaseBackend) {\n    let _ = connection.execute(Statement::from_string(backend, \"SELECT 1\".to_owned()));\n}\n",
    )
    .expect("write allowed tests subdir file");

    let report = scan_runtime_raw_sql_usage(&package_root, PACKAGE_PATH_PREFIX);
    let failure = std::panic::catch_unwind(|| assert_no_runtime_raw_sql_violations(&report))
        .expect_err("guard should panic for undocumented runtime raw SQL");
    let failure_message = panic_message(&failure);

    assert_eq!(
        report.violations.len(),
        2,
        "expected both injected runtime raw-SQL occurrences to be rejected, found:\n{}",
        report.violations.join("\n")
    );
    assert!(
        report.violations.iter().any(|violation| violation
            .contains("apps/axonhub-server/src/foundation/admin_operational.rs")),
        "missing admin_operational violation: {}",
        report.violations.join("\n")
    );
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.contains("Statement::from_string(")),
        "missing from_string entry-point detail: {}",
        report.violations.join("\n")
    );
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.contains("VACUUM")),
        "missing VACUUM violation: {}",
        report.violations.join("\n")
    );
    assert!(
        report.violations.iter().any(|violation| violation
            .contains("apps/axonhub-server/src/foundation/runtime_violation.rs")),
        "missing runtime_violation violation: {}",
        report.violations.join("\n")
    );
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.contains("Statement::from_sql_and_values(")),
        "missing from_sql_and_values entry-point detail: {}",
        report.violations.join("\n")
    );
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.contains("DELETE FROM systems WHERE id = $1")),
        "missing DELETE violation: {}",
        report.violations.join("\n")
    );
    assert!(
        failure_message.contains("apps/axonhub-server/src/foundation/admin_operational.rs"),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("VACUUM"),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("Statement::from_string("),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("apps/axonhub-server/src/foundation/runtime_violation.rs"),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("Statement::from_sql_and_values("),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("DELETE FROM systems WHERE id = $1"),
        "unexpected failure message: {failure_message}"
    );
}

#[test]
fn runtime_raw_sql_guard_rejects_multiple_occurrences_in_same_runtime_file() {
    let repo_root = unique_temp_dir("runtime-raw-sql-guard-duplicate");
    let package_root = repo_root.join(PACKAGE_PATH_PREFIX);
    let foundation_root = package_root.join(FOUNDATION_RUNTIME_ROOT);

    fs::create_dir_all(&foundation_root).expect("create foundation dir");

    fs::write(
        foundation_root.join("mod.rs"),
        "pub(crate) mod admin_operational;\n",
    )
    .expect("write foundation mod");
    fs::write(
        foundation_root.join("admin_operational.rs"),
        "fn run_gc_cleanup_now(connection: &Db) {\n    let _ = connection.execute(\n        Statement::from_string(\n            connection.get_database_backend(),\n            \"VACUUM\".to_owned(),\n        ),\n    );\n    let _ = connection.execute(\n        Statement::from_string(\n            connection.get_database_backend(),\n            \"VACUUM\".to_owned(),\n        ),\n    );\n}\n",
    )
    .expect("write duplicate violating runtime file");

    let report = scan_runtime_raw_sql_usage(&package_root, PACKAGE_PATH_PREFIX);
    let failure = std::panic::catch_unwind(|| assert_no_runtime_raw_sql_violations(&report))
        .expect_err("guard should panic for duplicated runtime raw SQL");
    let failure_message = panic_message(&failure);

    assert_eq!(
        report.violations.len(),
        2,
        "expected both duplicate runtime raw-SQL occurrences to be rejected, found:\n{}",
        report.violations.join("\n")
    );
    assert!(
        report.violations.iter().all(|violation| violation
            .contains("apps/axonhub-server/src/foundation/admin_operational.rs")),
        "unexpected violations: {}",
        report.violations.join("\n")
    );
    assert!(
        report
            .violations
            .iter()
            .all(|violation| violation.contains("Statement::from_string(")),
        "unexpected violations: {}",
        report.violations.join("\n")
    );
    assert!(
        report
            .violations
            .iter()
            .all(|violation| violation.contains("VACUUM")),
        "unexpected violations: {}",
        report.violations.join("\n")
    );
    assert!(
        failure_message.contains("apps/axonhub-server/src/foundation/admin_operational.rs"),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("VACUUM"),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("Statement::from_string("),
        "unexpected failure message: {failure_message}"
    );
}

#[test]
fn runtime_raw_sql_guard_ignores_nested_cfg_test_module_files() {
    let repo_root = unique_temp_dir("runtime-raw-sql-guard-nested-test-module");
    let package_root = repo_root.join(PACKAGE_PATH_PREFIX);
    let foundation_root = package_root.join(FOUNDATION_RUNTIME_ROOT);
    let system_test_support_root = foundation_root.join("system");

    fs::create_dir_all(&foundation_root).expect("create foundation dir");
    fs::create_dir_all(&system_test_support_root).expect("create nested system dir");

    fs::write(foundation_root.join("mod.rs"), "pub(crate) mod system;\n")
        .expect("write foundation mod");
    fs::write(
        foundation_root.join("system.rs"),
        "fn runtime_ok() {}\n\n#[cfg(test)]\npub(crate) mod sqlite_test_support;\n",
    )
    .expect("write runtime system module");
    fs::write(
        system_test_support_root.join("sqlite_test_support.rs"),
        "fn allowed_nested_test_support(connection: &Db, backend: DatabaseBackend) {\n    let _ = connection.execute(Statement::from_string(backend, \"SELECT 1\".to_owned()));\n}\n",
    )
    .expect("write nested cfg(test) module file");

    let report = scan_runtime_raw_sql_usage(&package_root, PACKAGE_PATH_PREFIX);

    assert!(
        report.violations.is_empty(),
        "nested cfg(test) module file should be ignored by runtime raw-SQL guard:\n{}",
        report.violations.join("\n")
    );
}

fn assert_runtime_raw_sql_target_contract() {
    assert_eq!(
        TARGET_RUNTIME_RAW_SQL_EXCEPTION_COUNT, 0,
        "final target contract drifted: runtime raw-SQL exceptions must stay at zero"
    );
}

fn assert_sqlite_support_suffix_target_contract() {
    assert_eq!(
        TARGET_SQLITE_SUPPORT_SUFFIX_FILE_COUNT, 0,
        "final target contract drifted: *sqlite_support.rs files must stay at zero"
    );
}

fn assert_no_runtime_raw_sql_violations(report: &RawSqlScanReport) {
    assert!(
        report.violations.is_empty(),
        "zero-runtime-raw-SQL policy rejected runtime raw SQL in production runtime paths:\n{}",
        report.violations.join("\n")
    );
}

fn assert_no_sqlite_support_suffix_files(found_files: &[String]) {
    assert!(
        found_files.is_empty(),
        "zero-suffix target drifted: *sqlite_support.rs files must stay deleted\nfound files:\n{}",
        found_files.join("\n")
    );
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }
    "non-string panic payload".to_owned()
}

fn scan_runtime_raw_sql_usage(package_root: &Path, display_prefix: &str) -> RawSqlScanReport {
    let mut report = RawSqlScanReport::default();

    for runtime_root in RUNTIME_SCAN_ROOTS.map(|relative| package_root.join(relative)) {
        let test_only_module_files = collect_test_only_module_files(&runtime_root);

        collect_rust_files(&runtime_root, &mut |path| {
            if is_explicit_test_file(path)
                || is_test_only_module_file(path, &test_only_module_files)
            {
                return;
            }

            let Ok(contents) = fs::read_to_string(path) else {
                return;
            };

            let inline_test_ranges = inline_cfg_test_module_ranges(&contents);
            let display_path = display_path(package_root, display_prefix, path);
            for occurrence in find_runtime_raw_sql_occurrences(&contents, &inline_test_ranges) {
                report.violations.push(format!(
                    "{}:{}: runtime raw SQL via {} -> {}",
                    display_path, occurrence.line, occurrence.entry_point, occurrence.fragment
                ));
            }
        });
    }

    report
}

fn scan_sqlite_support_suffix_files(package_root: &Path, display_prefix: &str) -> Vec<String> {
    let foundation_root = package_root.join(FOUNDATION_RUNTIME_ROOT);
    let mut files = Vec::new();

    collect_rust_files(&foundation_root, &mut |path| {
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|file_name| file_name.ends_with(SQLITE_SUPPORT_SUFFIX))
        {
            files.push(display_path(package_root, display_prefix, path));
        }
    });

    files.sort();
    files
}

fn display_path(package_root: &Path, display_prefix: &str, path: &Path) -> String {
    let relative = path.strip_prefix(package_root).unwrap_or(path);
    if display_prefix.is_empty() {
        relative.display().to_string()
    } else {
        format!("{display_prefix}/{}", relative.display())
    }
}

fn collect_test_only_module_files(runtime_root: &Path) -> BTreeSet<PathBuf> {
    let mut modules = BTreeSet::new();

    collect_rust_files(runtime_root, &mut |path| {
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };

        let inline_test_ranges = inline_cfg_test_module_ranges(&contents);
        let mut pending_cfg_test = false;

        for (index, line) in contents.lines().enumerate() {
            if line_is_in_ranges(index + 1, &inline_test_ranges) {
                continue;
            }

            let trimmed = line.trim();
            if trimmed.starts_with("#[cfg(test)]") {
                pending_cfg_test = true;
                continue;
            }

            if pending_cfg_test {
                if trimmed.is_empty() {
                    continue;
                }

                if let Some(module_name) = parse_module_decl(trimmed) {
                    modules.extend(resolve_cfg_test_module_files(path, module_name.as_str()));
                }
                pending_cfg_test = false;
            }
        }
    });

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

fn is_test_only_module_file(path: &Path, test_only_module_files: &BTreeSet<PathBuf>) -> bool {
    test_only_module_files.contains(path)
}

fn resolve_cfg_test_module_files(source_path: &Path, module_name: &str) -> Vec<PathBuf> {
    let parent_dir = source_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let base_dir = if source_path.file_name().and_then(|value| value.to_str()) == Some("mod.rs") {
        parent_dir
    } else {
        parent_dir.join(
            source_path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
        )
    };

    vec![
        base_dir.join(format!("{module_name}.rs")),
        base_dir.join(module_name).join("mod.rs"),
    ]
}

fn find_runtime_raw_sql_occurrences(
    contents: &str,
    inline_test_ranges: &[(usize, usize)],
) -> Vec<RawSqlOccurrence> {
    let normalized = normalized_runtime_source(contents, inline_test_ranges);
    let mut occurrences = Vec::new();

    for entry_point in RAW_SQL_ENTRY_PATTERNS {
        let mut search_start = 0;
        while let Some(relative_offset) = normalized.text[search_start..].find(entry_point) {
            let normalized_offset = search_start + relative_offset;
            let line = normalized.byte_lines[normalized_offset];
            occurrences.push(RawSqlOccurrence {
                entry_point,
                line,
                normalized_offset,
                fragment: snippet_from_line(contents, line),
            });
            search_start = normalized_offset + entry_point.len();
        }
    }

    occurrences.sort_by_key(|occurrence| occurrence.normalized_offset);
    occurrences
}

fn normalized_runtime_source(
    contents: &str,
    inline_test_ranges: &[(usize, usize)],
) -> NormalizedRuntimeSource {
    let mut normalized = NormalizedRuntimeSource::default();

    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if line_is_in_ranges(line_number, inline_test_ranges) || trimmed.starts_with("//") {
            continue;
        }

        for ch in line.chars() {
            if ch.is_whitespace() {
                continue;
            }

            normalized.text.push(ch);
            normalized
                .byte_lines
                .extend(std::iter::repeat(line_number).take(ch.len_utf8()));
        }
    }

    normalized
}

fn snippet_from_line(contents: &str, start_line: usize) -> String {
    const MAX_LINES: usize = 5;
    const MAX_CHARS: usize = 200;

    let mut fragment = contents
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(MAX_LINES)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if fragment.len() > MAX_CHARS {
        fragment.truncate(MAX_CHARS.saturating_sub(3));
        fragment.push_str("...");
    }

    fragment
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
