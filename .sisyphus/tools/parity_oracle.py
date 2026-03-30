#!/usr/bin/env python3
from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
PARITY_DIR = ROOT / ".sisyphus" / "parity"
SUITES_PATH = PARITY_DIR / "suites.json"
MANIFEST_VERSION = 1
FIXTURE_SCHEMA_VERSION = 1
_RUST_CLI_BINARY: Path | None = None


def load_manifest() -> dict[str, Any]:
    manifest = json.loads(SUITES_PATH.read_text(encoding="utf-8"))
    actual_version = manifest.get("manifest_version")
    if actual_version != MANIFEST_VERSION:
        raise SystemExit(
            f"unsupported parity manifest version: expected {MANIFEST_VERSION}, got {actual_version!r}"
        )
    return manifest


def suite_config(manifest: dict[str, Any], suite_name: str) -> dict[str, Any]:
    suites = manifest["suites"]
    if suite_name not in suites:
        raise SystemExit(f"unknown suite: {suite_name}")
    config = dict(suites[suite_name])
    config["name"] = suite_name
    config["fixture_path"] = PARITY_DIR / config["fixture"]
    return config


def load_fixture(config: dict[str, Any]) -> dict[str, Any]:
    fixture = json.loads(Path(config["fixture_path"]).read_text(encoding="utf-8"))
    actual_version = fixture.get("schema_version")
    if actual_version != FIXTURE_SCHEMA_VERSION:
        raise SystemExit(
            f"unsupported fixture schema version for {config['name']}: expected {FIXTURE_SCHEMA_VERSION}, got {actual_version!r}"
        )
    return fixture


def canonical_json(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def run_command(command: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    return subprocess.run(
        command,
        cwd=ROOT,
        env=merged_env,
        text=True,
        capture_output=True,
        check=False,
    )


def ensure_rust_cli_binary() -> Path:
    global _RUST_CLI_BINARY

    if _RUST_CLI_BINARY is not None:
        return _RUST_CLI_BINARY

    binary = ROOT / "target" / "debug" / "axonhub"
    build = run_command(["cargo", "build", "--quiet", "-p", "axonhub-server"])
    if build.returncode != 0:
        raise SystemExit(
            f"rust cli build failed\nSTDOUT:\n{build.stdout}\nSTDERR:\n{build.stderr}"
        )
    if not binary.exists():
        raise SystemExit(f"expected rust cli binary at {binary}")
    _RUST_CLI_BINARY = binary
    return _RUST_CLI_BINARY


def emit_cli_output(runtime: str, config: dict[str, Any], fixture: dict[str, Any]) -> dict[str, Any]:
    args = fixture.get("args", [])
    env = fixture.get("env")
    if env is not None and not isinstance(env, dict):
        raise SystemExit(f"cli fixture env for {config['name']} must be an object")
    if runtime == "go":
        command = ["go", "run", "./cmd/axonhub", *args]
    elif runtime == "rust":
        command = [str(ensure_rust_cli_binary()), *args]
    else:
        raise SystemExit(f"unsupported runtime: {runtime}")

    result = run_command(command, env=env)
    normalized_stderr = result.stderr.replace("\r\n", "\n")
    normalized_stderr = re.sub(r"(?m)^go: downloading .*$\n?", "", normalized_stderr)
    normalized_stderr = re.sub(r"(?m)^exit status \d+\n?", "", normalized_stderr)
    if result.returncode == 0:
        normalized_stderr = ""
    return {
        "suite": config["name"],
        "kind": "cli",
        "exit_code": result.returncode,
        "stdout": result.stdout.replace("\r\n", "\n"),
        "stderr": normalized_stderr,
    }


def emit_oracle_test_output(runtime: str, config: dict[str, Any]) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix=f"axonhub-parity-{config['name']}-") as temp_dir:
        capture_path = Path(temp_dir) / f"{runtime}.json"
        env = {
            "AXONHUB_PARITY_SUITE": config["name"],
            "AXONHUB_PARITY_FIXTURE": str(config["fixture_path"]),
            "AXONHUB_PARITY_CAPTURE": str(capture_path),
        }
        if runtime == "go":
            command = [
                "go",
                "test",
                "./.sisyphus/parity",
                "-run",
                "^TestParityOracleEmitSuite$",
                "-count=1",
            ]
        elif runtime == "rust":
            command = [
                "cargo",
                "test",
                "-p",
                "axonhub-server",
                "parity_oracle_emit_suite",
                "--",
                "--nocapture",
            ]
        else:
            raise SystemExit(f"unsupported runtime: {runtime}")

        result = run_command(command, env=env)
        if result.returncode != 0:
            raise SystemExit(
                f"{runtime} oracle execution failed for {config['name']}\nSTDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
            )
        if not capture_path.exists():
            raise SystemExit(f"{runtime} oracle execution did not write {capture_path}")
        return json.loads(capture_path.read_text(encoding="utf-8"))


def _normalize_command_checks(config: dict[str, Any], fixture: dict[str, Any]) -> list[dict[str, Any]]:
    commands = fixture.get("commands")
    if commands is not None:
        if not isinstance(commands, list):
            raise SystemExit(f"command fixture for {config['name']} must provide a list under 'commands'")
        normalized: list[dict[str, Any]] = []
        for index, entry in enumerate(commands, start=1):
            if not isinstance(entry, dict):
                raise SystemExit(f"command fixture entry {index} for {config['name']} must be an object")
            command = entry.get("command")
            name = entry.get("name", f"check-{index}")
            if not isinstance(name, str):
                raise SystemExit(f"command fixture entry {index} for {config['name']} has invalid name")
            if not isinstance(command, list) or not all(isinstance(item, str) for item in command):
                raise SystemExit(f"command fixture entry {index} for {config['name']} must provide a string-list command")
            normalized.append({"name": name, "command": command})
        return normalized

    command = fixture.get("command")
    if not isinstance(command, list) or not all(isinstance(item, str) for item in command):
        raise SystemExit(f"command fixture for {config['name']} must provide a string list")
    return [{"name": config["name"], "command": command}]


def emit_command_output(config: dict[str, Any], fixture: dict[str, Any]) -> dict[str, Any]:
    checks = []
    exit_code = 0
    for check in _normalize_command_checks(config, fixture):
        result = run_command(check["command"])
        exit_code = max(exit_code, result.returncode)
        checks.append(
            {
                "name": check["name"],
                "command": check["command"],
                "exit_code": result.returncode,
                "stdout": result.stdout.replace("\r\n", "\n"),
                "stderr": result.stderr.replace("\r\n", "\n"),
            }
        )

    return {
        "suite": config["name"],
        "kind": "command",
        "exit_code": exit_code,
        "checks": checks,
    }


def emit_runtime_output(runtime: str, config: dict[str, Any]) -> dict[str, Any]:
    fixture = load_fixture(config)
    if config["kind"] == "cli":
        return emit_cli_output(runtime, config, fixture)
    if config["kind"] == "oracle_test":
        return emit_oracle_test_output(runtime, config)
    if config["kind"] == "command":
        return emit_command_output(config, fixture)
    raise SystemExit(f"unsupported suite kind: {config['kind']}")


def unified_diff(left_name: str, left: str, right_name: str, right: str) -> str:
    return "".join(
        difflib.unified_diff(
            left.splitlines(keepends=True),
            right.splitlines(keepends=True),
            fromfile=left_name,
            tofile=right_name,
        )
    )


def compare_suite(config: dict[str, Any], diff_out: Path | None = None) -> tuple[bool, str]:
    if config["kind"] == "command":
        command_output = emit_runtime_output("host", config)
        rendered = canonical_json(command_output)
        if diff_out is not None:
            diff_out.parent.mkdir(parents=True, exist_ok=True)
            diff_out.write_text(rendered, encoding="utf-8")
        if command_output["exit_code"] == 0:
            return True, ""
        return False, rendered

    go_output = emit_runtime_output("go", config)
    rust_output = emit_runtime_output("rust", config)
    go_rendered = canonical_json(go_output)
    rust_rendered = canonical_json(rust_output)
    matched = go_rendered == rust_rendered
    expected_parity = bool(config["expected_parity"])
    diff_text = "" if matched else unified_diff(f"go:{config['name']}", go_rendered, f"rust:{config['name']}", rust_rendered)

    if diff_out is not None:
        diff_out.parent.mkdir(parents=True, exist_ok=True)
        payload = diff_text if diff_text else f"suite {config['name']} matched with no diff\n"
        diff_out.write_text(payload, encoding="utf-8")

    if expected_parity and not matched:
        return False, diff_text
    if not expected_parity and matched:
        return False, f"suite {config['name']} unexpectedly matched; expected a deterministic mismatch\n"
    return True, diff_text


def resolve_suite_names(manifest: dict[str, Any], suite_set: str | None, explicit_suites: list[str] | None) -> list[str]:
    if explicit_suites:
        return explicit_suites
    if suite_set:
        try:
            return list(manifest["suite_sets"][suite_set])
        except KeyError as error:
            raise SystemExit(f"unknown suite set: {suite_set}") from error
    raise SystemExit("either --suite or --suite-set is required")


def command_list(manifest: dict[str, Any]) -> int:
    for name, config in manifest["suites"].items():
        expected = "match" if config["expected_parity"] else "mismatch"
        print(f"{name}\t{config['family']}\t{expected}\t{config['description']}")
    return 0


def command_emit(manifest: dict[str, Any], suite_name: str, runtime: str, output: Path | None) -> int:
    config = suite_config(manifest, suite_name)
    rendered = canonical_json(emit_runtime_output(runtime, config))
    if output is None:
        sys.stdout.write(rendered)
    else:
        output = output if output.is_absolute() else ROOT / output
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8")
        print(f"wrote {output.relative_to(ROOT)}")
    return 0


def command_compare(manifest: dict[str, Any], suite_name: str, diff_out: Path | None) -> int:
    config = suite_config(manifest, suite_name)
    ok, diff_text = compare_suite(config, diff_out)
    if ok:
        verdict = "matched" if config["expected_parity"] else "mismatched as expected"
        print(f"suite {suite_name} {verdict}")
        return 0
    if diff_text:
        sys.stderr.write(diff_text)
    return 1


def command_compare_set(
    manifest: dict[str, Any],
    suite_set: str | None,
    suites: list[str] | None,
    diff_dir: Path | None,
) -> int:
    suite_names = resolve_suite_names(manifest, suite_set, suites)
    failures: list[str] = []
    for suite_name in suite_names:
        config = suite_config(manifest, suite_name)
        diff_out = diff_dir / f"{suite_name}.diff" if diff_dir else None
        ok, diff_text = compare_suite(config, diff_out)
        if ok:
            print(f"suite {suite_name} ok")
            continue
        failures.append(suite_name)
        if diff_text:
            sys.stderr.write(diff_text)
    if failures:
        print(f"failed suites: {', '.join(failures)}", file=sys.stderr)
        return 1
    return 0


def command_stability(
    manifest: dict[str, Any],
    suite_set: str | None,
    suites: list[str] | None,
    output: Path,
) -> int:
    suite_names = resolve_suite_names(manifest, suite_set, suites)

    def capture_pass() -> dict[str, Any]:
        snapshot: dict[str, Any] = {}
        for suite_name in suite_names:
            config = suite_config(manifest, suite_name)
            snapshot[suite_name] = {
                "go": emit_runtime_output("go", config),
                "rust": emit_runtime_output("rust", config),
            }
        return snapshot

    first = capture_pass()
    second = capture_pass()
    first_rendered = canonical_json(first)
    second_rendered = canonical_json(second)
    output = output if output.is_absolute() else ROOT / output
    output.parent.mkdir(parents=True, exist_ok=True)
    if first_rendered == second_rendered:
        output.write_text(
            f"stable suite set: {suite_set or ','.join(suite_names)}\n"
            f"suite count: {len(suite_names)}\n"
            f"result: byte-stable across two consecutive runs\n",
            encoding="utf-8",
        )
        print(f"wrote {output.relative_to(ROOT)}")
        return 0

    output.write_text(
        unified_diff("first-pass", first_rendered, "second-pass", second_rendered),
        encoding="utf-8",
    )
    print(f"wrote unstable diff to {output.relative_to(ROOT)}", file=sys.stderr)
    return 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run named Go-vs-Rust parity oracle suites")
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("list", help="List available suites")

    emit_parser = subparsers.add_parser("emit", help="Emit normalized output for one runtime")
    emit_parser.add_argument("--suite", required=True)
    emit_parser.add_argument("--runtime", required=True, choices=["go", "rust"])
    emit_parser.add_argument("--output", type=Path)

    compare_parser = subparsers.add_parser("compare", help="Compare one suite")
    compare_parser.add_argument("--suite", required=True)
    compare_parser.add_argument("--diff-out", type=Path)

    compare_set_parser = subparsers.add_parser("compare-set", help="Compare a suite set")
    compare_set_parser.add_argument("--suite-set")
    compare_set_parser.add_argument("--suite", action="append")
    compare_set_parser.add_argument("--diff-dir", type=Path)

    stability_parser = subparsers.add_parser("stability", help="Verify byte-stable outputs across two runs")
    stability_parser.add_argument("--suite-set")
    stability_parser.add_argument("--suite", action="append")
    stability_parser.add_argument("--output", required=True, type=Path)

    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest = load_manifest()
    if args.command == "list":
        return command_list(manifest)
    if args.command == "emit":
        return command_emit(manifest, args.suite, args.runtime, args.output)
    if args.command == "compare":
        return command_compare(manifest, args.suite, args.diff_out)
    if args.command == "compare-set":
        return command_compare_set(manifest, args.suite_set, args.suite, args.diff_dir)
    if args.command == "stability":
        return command_stability(manifest, args.suite_set, args.suite, args.output)
    raise SystemExit(f"unsupported command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
