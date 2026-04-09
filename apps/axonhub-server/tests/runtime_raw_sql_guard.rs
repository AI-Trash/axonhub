use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PACKAGE_PATH_PREFIX: &str = "apps/axonhub-server";
const FOUNDATION_RUNTIME_ROOT: &str = "src/foundation";
const RUNTIME_SCAN_ROOTS: [&str; 2] = ["src/app", FOUNDATION_RUNTIME_ROOT];
const RAW_SQL_ENTRY_POINT: &str = "Statement::from_string(";
const SQLITE_SUPPORT_SUFFIX: &str = "sqlite_support.rs";
const TARGET_RUNTIME_RAW_SQL_EXCEPTION_COUNT: usize = 0;
const TARGET_SQLITE_SUPPORT_SUFFIX_FILE_COUNT: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TrackedRuntimeRawSqlDebt {
    id: &'static str,
    path: &'static str,
    line_fragment: &'static str,
    removal_target: &'static str,
}

const TRACKED_RUNTIME_RAW_SQL_DEBT: [TrackedRuntimeRawSqlDebt; 1] = [TrackedRuntimeRawSqlDebt {
    id: "OperationalMaintenanceCommandVacuumDebt",
    path: "apps/axonhub-server/src/foundation/admin_operational.rs",
    line_fragment:
        "Statement::from_string(connection.get_database_backend(), \"VACUUM\".to_owned())",
    removal_target: "Task 11 removes the runtime VACUUM execution path entirely",
}];

const TRACKED_SQLITE_SUPPORT_SUFFIX_FILES: [&str; 6] = [
    "apps/axonhub-server/src/foundation/admin_sqlite_support.rs",
    "apps/axonhub-server/src/foundation/graphql_sqlite_support.rs",
    "apps/axonhub-server/src/foundation/identity_sqlite_support.rs",
    "apps/axonhub-server/src/foundation/openai_v1_sqlite_support.rs",
    "apps/axonhub-server/src/foundation/request_context_sqlite_support.rs",
    "apps/axonhub-server/src/foundation/sqlite_support.rs",
];

#[derive(Debug, Default)]
struct RawSqlScanReport {
    tracked_debt_ids: BTreeSet<&'static str>,
    tracked_debt_occurrences: Vec<String>,
    violations: Vec<String>,
}

#[test]
fn runtime_raw_sql_target_contract_requires_zero_exceptions() {
    assert_runtime_raw_sql_target_contract();
}

#[test]
fn runtime_raw_sql_guard_tracks_remaining_debt_until_zero_exception_target() {
    assert_runtime_raw_sql_target_contract();

    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = scan_runtime_raw_sql_usage(&package_root, PACKAGE_PATH_PREFIX);
    let expected = TRACKED_RUNTIME_RAW_SQL_DEBT
        .iter()
        .map(|debt| debt.id)
        .collect::<BTreeSet<_>>();

    assert_no_untracked_runtime_raw_sql_violations(&report);
    assert_eq!(
        report.tracked_debt_ids,
        expected,
        "zero-runtime-raw-SQL target drifted: the tracked transition debt snapshot changed\ntracked debt occurrences:\n{}",
        report.tracked_debt_occurrences.join("\n")
    );
    assert_eq!(
        report.tracked_debt_occurrences.len(),
        TRACKED_RUNTIME_RAW_SQL_DEBT.len(),
        "zero-runtime-raw-SQL target still allows no exceptions; expected exactly {} tracked transition debt occurrence(s), found {}\ntracked debt occurrences:\n{}",
        TRACKED_RUNTIME_RAW_SQL_DEBT.len(),
        report.tracked_debt_occurrences.len(),
        report.tracked_debt_occurrences.join("\n")
    );
}

#[test]
fn runtime_raw_sql_count_reduced_or_removed() {
    runtime_raw_sql_guard_tracks_remaining_debt_until_zero_exception_target();
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
    let mut expected_files = TRACKED_SQLITE_SUPPORT_SUFFIX_FILES
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    expected_files.sort();

    assert_eq!(
        found_files,
        expected_files,
        "zero-suffix target drifted: expected only the tracked transition files to remain before deletion work begins\nfound files:\n{}",
        found_files.join("\n")
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
        "pub(crate) mod seaorm;\npub(crate) mod admin_operational;\npub(crate) mod runtime_violation;\n#[cfg(test)]\npub(crate) mod sqlite_support;\n",
    )
    .expect("write foundation mod");
    fs::write(
        foundation_root.join("admin_operational.rs"),
        "fn run_gc_cleanup_now(connection: &Db) {\n    let _ = connection.execute(Statement::from_string(connection.get_database_backend(), \"VACUUM\".to_owned()));\n}\n",
    )
    .expect("write allowed operational vacuum");
    fs::write(
        foundation_root.join("runtime_violation.rs"),
        "fn undocumented(connection: &Db, backend: DatabaseBackend) {\n    let _ = connection.execute(Statement::from_string(backend, \"DELETE FROM systems\".to_owned()));\n}\n\n#[cfg(test)]\nmod tests {\n    fn allowed_only_in_cfg_test(connection: &Db, backend: DatabaseBackend) {\n        let _ = connection.execute(Statement::from_string(backend, \"SELECT 1\".to_owned()));\n    }\n}\n",
    )
    .expect("write violating runtime file");
    fs::write(
        foundation_root.join("sqlite_support.rs"),
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
    let failure =
        std::panic::catch_unwind(|| assert_no_untracked_runtime_raw_sql_violations(&report))
            .expect_err("guard should panic for undocumented runtime raw SQL");
    let failure_message = panic_message(&failure);

    assert_eq!(
        report.tracked_debt_ids,
        TRACKED_RUNTIME_RAW_SQL_DEBT
            .iter()
            .map(|debt| debt.id)
            .collect::<BTreeSet<_>>(),
        "expected tracked transition debt fixtures to be discovered"
    );
    assert_eq!(
        report.violations.len(),
        1,
        "expected exactly one undocumented runtime raw-SQL occurrence, found:\n{}",
        report.violations.join("\n")
    );
    assert!(
        report.violations[0].contains("apps/axonhub-server/src/foundation/runtime_violation.rs"),
        "unexpected violation: {}",
        report.violations[0]
    );
    assert!(
        report.violations[0].contains("DELETE FROM systems"),
        "unexpected violation: {}",
        report.violations[0]
    );
    assert!(
        failure_message.contains("apps/axonhub-server/src/foundation/runtime_violation.rs"),
        "unexpected failure message: {failure_message}"
    );
    assert!(
        failure_message.contains("DELETE FROM systems"),
        "unexpected failure message: {failure_message}"
    );
}

#[test]
fn runtime_raw_sql_guard_rejects_duplicate_tracked_debt_occurrence() {
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
        "fn run_gc_cleanup_now(connection: &Db) {\n    let _ = connection.execute(Statement::from_string(connection.get_database_backend(), \"VACUUM\".to_owned()));\n    let _ = connection.execute(Statement::from_string(connection.get_database_backend(), \"VACUUM\".to_owned()));\n}\n",
    )
    .expect("write duplicate allowed boundary");

    let report = scan_runtime_raw_sql_usage(&package_root, PACKAGE_PATH_PREFIX);
    let failure =
        std::panic::catch_unwind(|| assert_no_untracked_runtime_raw_sql_violations(&report))
            .expect_err("guard should panic for duplicated tracked runtime raw SQL debt");
    let failure_message = panic_message(&failure);

    assert_eq!(
        report.tracked_debt_ids,
        TRACKED_RUNTIME_RAW_SQL_DEBT
            .iter()
            .map(|debt| debt.id)
            .collect::<BTreeSet<_>>(),
        "expected duplicate fixture to discover the tracked debt boundary before rejecting the duplicate"
    );
    assert_eq!(
        report.violations.len(),
        1,
        "expected exactly one duplicate tracked-debt violation, found:\n{}",
        report.violations.join("\n")
    );
    assert!(
        report.violations[0].contains(
            "duplicate tracked runtime raw-SQL debt `OperationalMaintenanceCommandVacuumDebt`"
        ),
        "unexpected violation: {}",
        report.violations[0]
    );
    assert!(
        failure_message.contains(
            "duplicate tracked runtime raw-SQL debt `OperationalMaintenanceCommandVacuumDebt`"
        ),
        "unexpected failure message: {failure_message}"
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

fn assert_no_untracked_runtime_raw_sql_violations(report: &RawSqlScanReport) {
    assert!(
        report.violations.is_empty(),
        "zero-runtime-raw-SQL target rejected runtime raw SQL outside the tracked transition snapshot:\n{}",
        report.violations.join("\n")
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
        let test_only_modules = collect_test_only_modules(&runtime_root);

        collect_rust_files(&runtime_root, &mut |path| {
            if is_explicit_test_file(path) || is_test_only_module_file(path, &test_only_modules) {
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

                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || !trimmed.contains(RAW_SQL_ENTRY_POINT) {
                    continue;
                }

                let display_path = display_path(package_root, display_prefix, path);
                if let Some(debt) = TRACKED_RUNTIME_RAW_SQL_DEBT
                    .iter()
                    .find(|debt| debt.path == display_path && trimmed.contains(debt.line_fragment))
                {
                    if report.tracked_debt_ids.insert(debt.id) {
                        report.tracked_debt_occurrences.push(format!(
                            "{}:{}: tracked `{}` ({})",
                            display_path,
                            index + 1,
                            debt.id,
                            debt.removal_target
                        ));
                    } else {
                        report.violations.push(format!(
                            "{}:{}: duplicate tracked runtime raw-SQL debt `{}`",
                            display_path,
                            index + 1,
                            debt.id
                        ));
                    }
                } else {
                    report.violations.push(format!(
                        "{}:{}: undocumented runtime raw SQL entry point -> {}",
                        display_path,
                        index + 1,
                        trimmed
                    ));
                }
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

fn collect_test_only_modules(foundation_root: &Path) -> BTreeSet<String> {
    let mut modules = BTreeSet::new();
    let mod_rs = foundation_root.join("mod.rs");
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
    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
        if test_only_modules.contains(stem) {
            return true;
        }
    }

    path.file_name().and_then(|value| value.to_str()) == Some("mod.rs")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|value| value.to_str())
            .is_some_and(|module_name| test_only_modules.contains(module_name))
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
