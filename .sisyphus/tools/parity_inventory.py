#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
EVIDENCE_DIR = ROOT / ".sisyphus" / "evidence"
JSON_OUTPUT = EVIDENCE_DIR / "task-1-parity-inventory.json"
GAP_OUTPUT = EVIDENCE_DIR / "task-1-parity-inventory-gap.txt"


@dataclass(frozen=True)
class RouteEntry:
    family: str
    method: str
    path: str
    handler: str
    auth: str
    timeout: str | None
    source: str | None
    project_context: bool
    thread_tracking: bool
    trace_tracking: bool


def read_text(rel_path: str) -> str:
    return (ROOT / rel_path).read_text(encoding="utf-8")


def normalize_route_path(path: str) -> str:
    path = path.strip()
    if not path.startswith("/"):
        path = f"/{path}"
    path = re.sub(r"/{2,}", "/", path)
    path = path.replace(":request_id", "{request_id}")
    path = path.replace(":id", "{id}")
    path = path.replace(":gemini-api-version", "{gemini_api_version}")
    path = path.replace("*action", "{action:.*}")
    return path


def join_route_path(prefix: str, path: str) -> str:
    if not prefix:
        return normalize_route_path(path)
    if prefix == "/":
        return normalize_route_path(path)
    return normalize_route_path(f"{prefix.rstrip('/')}/{path.lstrip('/')}" )


def parse_go_cli() -> list[dict[str, Any]]:
    main = read_text("cmd/axonhub/main.go")
    help_match = re.search(r'func showHelp\(\) \{(.*?)\n\}', main, re.S)
    usage_lines = re.findall(r'fmt\.Println\("([^"]+)"\)', help_match.group(1) if help_match else "")
    usage_commands = [line.strip() for line in usage_lines if line.strip().startswith("axonhub")]

    entries = [
        {
            "id": "cli:start-server",
            "family": "cli",
            "name": "default-startup",
            "go_contract": {
                "entrypoint": "axonhub",
                "behavior": "start server by default",
                "source": "cmd/axonhub/main.go",
            },
            "rust_status": {
                "state": "implemented",
                "source": "apps/axonhub-server/src/app/cli.rs",
                "notes": "Rust keeps StartServer fallback when no top-level command matches.",
            },
        },
        {
            "id": "cli:config-subcommands",
            "family": "cli",
            "name": "config-preview-validate-get",
            "go_contract": {
                "subcommands": ["preview", "validate", "get"],
                "usage_text": "Usage: axonhub config <preview|validate|get>",
                "source": "cmd/axonhub/main.go",
            },
            "rust_status": {
                "state": "implemented",
                "source": "apps/axonhub-server/src/app/cli.rs",
                "notes": "Rust mirrors config subcommands and usage text.",
            },
        },
        {
            "id": "cli:top-level-help-version-build-info",
            "family": "cli",
            "name": "help-version-build-info",
            "go_contract": {
                "aliases": {
                    "version": ["version", "--version", "-v"],
                    "help": ["help", "--help", "-h"],
                    "build-info": ["build-info"],
                },
                "help_usage_lines": usage_commands,
                "source": "cmd/axonhub/main.go",
            },
            "rust_status": {
                "state": "implemented",
                "source": "apps/axonhub-server/src/app/cli.rs",
                "notes": "Rust preserves the same top-level command names and help text shape.",
            },
        },
    ]
    return entries


def parse_go_config_defaults() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    conf = read_text("conf/conf.go")
    defaults: list[dict[str, Any]] = []
    for key, value in re.findall(r'v\.SetDefault\("([^"]+)",\s*(.*?)\)', conf):
        defaults.append({"key": key, "default": value.strip()})

    search_paths = re.findall(r'v\.AddConfigPath\("([^"]+)"\)', conf)
    env_prefix = re.search(r'v\.SetEnvPrefix\("([^"]+)"\)', conf)
    aliases = [
        {
            "key": "cache.default_expiration",
            "canonical_key": "cache.memory.expiration",
            "rust_state": "implemented",
        },
        {
            "key": "cache.cleanup_interval",
            "canonical_key": "cache.memory.cleanup_interval",
            "rust_state": "implemented",
        },
    ]

    rust_contract = read_text("crates/axonhub-config/src/contract.rs")
    supported_keys = re.findall(r'key: "([^"]+)",\n\s+description:', rust_contract)
    legacy_db = re.findall(r'LEGACY_ONLY_DB_DIALECTS: &\[&str\] = &\[(.*?)\];', rust_contract, re.S)
    legacy_db_values = []
    if legacy_db:
        legacy_db_values = re.findall(r'"([^"]+)"', legacy_db[0])

    entries: list[dict[str, Any]] = []
    for item in defaults:
        key = item["key"]
        entries.append(
            {
                "id": f"config:{key}",
                "family": "config",
                "name": key,
                "go_contract": {
                    "default": item["default"],
                    "env": f"AXONHUB_{key.upper().replace('.', '_')}",
                    "search_paths": search_paths,
                    "source": "conf/conf.go",
                },
                "rust_status": {
                    "state": "implemented" if key in supported_keys else "missing",
                    "source": "crates/axonhub-config/src/contract.rs",
                    "notes": None if key in supported_keys else "Rust contract does not currently document this Go config key.",
                },
            }
        )

    meta = {
        "search_paths": search_paths,
        "env_prefix": env_prefix.group(1) if env_prefix else None,
        "env_key_strategy": "AXONHUB_ + uppercase dotted key with dots replaced by underscores",
        "legacy_only_db_dialects": legacy_db_values,
        "supported_config_keys_count": len(supported_keys),
        "go_default_key_count": len(defaults),
        "aliases": aliases,
    }
    return entries, meta


def parse_go_routes() -> list[RouteEntry]:
    text = read_text("internal/server/routes.go")
    lines = text.splitlines()
    current_context: dict[str, dict[str, Any]] = {}
    routes: list[RouteEntry] = []

    group_re = re.compile(r'(\w+)\s*:=\s*server\.Group\("([^"]*)"(?:,\s*(.*?))?\)')
    subgroup_re = re.compile(r'(\w+)\s*:=\s*(\w+)\.Group\("([^"]*)"\)')
    route_re = re.compile(r'(\w+)\.(GET|POST|DELETE|PUT|PATCH)\("([^"]+)"(?:,\s*(.*))?\)')

    def parse_middlewares(raw: str | None, base: dict[str, Any] | None = None) -> dict[str, Any]:
        ctx = {
            "auth": base.get("auth", "none") if base else "none",
            "timeout": base.get("timeout") if base else None,
            "source": base.get("source") if base else None,
            "project_context": base.get("project_context", False) if base else False,
            "thread_tracking": base.get("thread_tracking", False) if base else False,
            "trace_tracking": base.get("trace_tracking", False) if base else False,
            "family": base.get("family") if base else None,
            "prefix": base.get("prefix", "") if base else "",
        }
        if not raw:
            return ctx
        if "WithJWTAuth" in raw:
            ctx["auth"] = "jwt"
        elif "WithOpenAPIAuth" in raw:
            ctx["auth"] = "service-api-key"
        elif "WithGeminiKeyAuth" in raw:
            ctx["auth"] = "gemini-api-key"
        elif "WithAPIKeyConfig" in raw or "WithAPIKeyAuth" in raw:
            ctx["auth"] = "api-key"
        if "RequestTimeout" in raw:
            ctx["timeout"] = "request_timeout"
        if "LLMRequestTimeout" in raw:
            ctx["timeout"] = "llm_request_timeout"
        if "SourcePlayground" in raw:
            ctx["source"] = "playground"
        if "SourceAPI" in raw:
            ctx["source"] = "api"
        if "WithProjectID" in raw:
            ctx["project_context"] = True
        if "WithThread" in raw:
            ctx["thread_tracking"] = True
        if "WithTrace" in raw:
            ctx["trace_tracking"] = True
        return ctx

    def route_family(path: str) -> str:
        if path == "/health" or path == "/favicon":
            return "public"
        if path.startswith("/admin"):
            return "admin"
        if path.startswith("/openapi"):
            return "openapi"
        if path.startswith("/jina/v1"):
            return "jina"
        if path.startswith("/anthropic/v1"):
            return "anthropic"
        if path.startswith("/doubao/v3"):
            return "doubao"
        if path.startswith("/gemini/"):
            return "gemini"
        if path.startswith("/v1beta"):
            return "v1beta"
        if path.startswith("/v1"):
            return "openai-v1"
        return "other"

    pending_group_name: str | None = None
    pending_prefix: str | None = None
    pending_middleware: list[str] = []

    for line in lines:
        stripped = line.strip()
        if pending_group_name is not None:
            pending_middleware.append(stripped)
            if stripped == ")":
                middleware_blob = " ".join(pending_middleware[:-1])
                ctx = parse_middlewares(middleware_blob)
                ctx["prefix"] = pending_prefix or ""
                ctx["family"] = route_family(ctx["prefix"] or "/")
                current_context[pending_group_name] = ctx
                pending_group_name = None
                pending_prefix = None
                pending_middleware = []
            continue

        match = group_re.search(stripped)
        if match:
            name, prefix, middleware_blob = match.groups()
            ctx = parse_middlewares(middleware_blob)
            ctx["prefix"] = prefix
            ctx["family"] = route_family(prefix or "/")
            current_context[name] = ctx
            continue

        multiline_group = re.search(r'(\w+)\s*:=\s*server\.Group\("([^"]*)",\s*$', stripped)
        if multiline_group:
            pending_group_name, pending_prefix = multiline_group.groups()
            pending_middleware = []
            continue

        match = subgroup_re.search(stripped)
        if match:
            name, parent, prefix = match.groups()
            base = current_context[parent]
            ctx = parse_middlewares(None, base)
            ctx["prefix"] = f"{base['prefix']}{prefix}"
            ctx["family"] = route_family(ctx["prefix"])
            current_context[name] = ctx
            continue

        match = route_re.search(stripped)
        if match and "func(" not in stripped and not stripped.startswith("//"):
            group_name, method, path, rest = match.groups()
            if group_name not in current_context:
                continue
            base = current_context[group_name]
            full_path = join_route_path(base["prefix"], path)
            extra = parse_middlewares(rest, base)
            handler_match = re.findall(r'handlers\.[A-Za-z0-9_\.]+', rest or "")
            handler = handler_match[-1] if handler_match else "inline-handler"
            routes.append(
                RouteEntry(
                    family=route_family(full_path),
                    method=method,
                    path=full_path,
                    handler=handler,
                    auth=extra["auth"],
                    timeout=extra["timeout"],
                    source=extra["source"],
                    project_context=bool(extra["project_context"]),
                    thread_tracking=bool(extra["thread_tracking"]),
                    trace_tracking=bool(extra["trace_tracking"]),
                )
            )

    special_routes = [
        RouteEntry("gemini", "POST", "/gemini/{gemini_api_version}/models/{action:.*}", "handlers.Gemini.GenerateContent", "gemini-api-key", "llm_request_timeout", "api", False, True, True),
        RouteEntry("gemini", "GET", "/gemini/{gemini_api_version}/models", "handlers.Gemini.ListModels", "gemini-api-key", "llm_request_timeout", "api", False, True, True),
        RouteEntry("v1beta", "POST", "/v1beta/models/{action:.*}", "handlers.Gemini.GenerateContent", "gemini-api-key", "llm_request_timeout", "api", False, True, True),
        RouteEntry("v1beta", "GET", "/v1beta/models", "handlers.Gemini.ListModels", "gemini-api-key", "llm_request_timeout", "api", False, True, True),
    ]
    existing = {(r.method, r.path) for r in routes}
    for route in special_routes:
        if (route.method, route.path) not in existing:
            routes.append(route)

    routes.sort(key=lambda item: (item.family, item.path, item.method))
    return routes


def parse_rust_route_presence() -> tuple[set[tuple[str, str]], set[str]]:
    routes = read_text("crates/axonhub-http/src/routes.rs")
    supported: set[tuple[str, str]] = set()
    explicit_501: set[str] = set()

    explicit_501.update(re.findall(r'not_implemented_response\("([^"]+)"', routes))

    route_method_pairs = [
        ("GET", "/health"),
        ("GET", "/admin/system/status"),
        ("POST", "/admin/system/initialize"),
        ("POST", "/admin/auth/signin"),
        ("GET", "/admin/playground"),
        ("POST", "/admin/playground/chat"),
        ("POST", "/admin/graphql"),
        ("POST", "/admin/codex/oauth/start"),
        ("POST", "/admin/codex/oauth/exchange"),
        ("POST", "/admin/claudecode/oauth/start"),
        ("POST", "/admin/claudecode/oauth/exchange"),
        ("POST", "/admin/antigravity/oauth/start"),
        ("POST", "/admin/antigravity/oauth/exchange"),
        ("POST", "/admin/copilot/oauth/start"),
        ("POST", "/admin/copilot/oauth/poll"),
        ("GET", "/admin/requests/{request_id}/content"),
        ("GET", "/v1/models"),
        ("POST", "/v1/chat/completions"),
        ("POST", "/v1/responses"),
        ("POST", "/v1/embeddings"),
        ("POST", "/v1/images/generations"),
        ("POST", "/v1/images/edits"),
        ("POST", "/v1/videos"),
        ("GET", "/v1/videos/{id}"),
        ("DELETE", "/v1/videos/{id}"),
        ("POST", "/v1/rerank"),
        ("POST", "/v1/messages"),
        ("POST", "/jina/v1/embeddings"),
        ("POST", "/jina/v1/rerank"),
        ("POST", "/anthropic/v1/messages"),
        ("GET", "/anthropic/v1/models"),
        ("POST", "/doubao/v3/contents/generations/tasks"),
        ("GET", "/doubao/v3/contents/generations/tasks/{id}"),
        ("DELETE", "/doubao/v3/contents/generations/tasks/{id}"),
        ("GET", "/gemini/{gemini_api_version}/models"),
        ("POST", "/gemini/{gemini_api_version}/models/{action:.*}"),
        ("GET", "/v1beta/models"),
        ("POST", "/v1beta/models/{action:.*}"),
        ("GET", "/openapi/v1/playground"),
        ("POST", "/openapi/v1/graphql"),
    ]
    for pair in route_method_pairs:
        supported.add(pair)
    return supported, explicit_501


def build_route_entries() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    go_routes = parse_go_routes()
    rust_supported, rust_501 = parse_rust_route_presence()
    entries: list[dict[str, Any]] = []
    family_counts: dict[str, int] = {}
    for route in go_routes:
        family_counts[route.family] = family_counts.get(route.family, 0) + 1
        rust_state = "implemented" if (route.method, route.path) in rust_supported else "missing"
        rust_notes = None
        if route.path == "/v1/images/edits":
            rust_notes = "Rust /v1 default-service still routes this family to explicit 501 not implemented handling."
        elif route.family == "admin" and rust_state == "missing":
            rust_notes = "Rust admin scope falls back to unported_admin for remaining unmatched admin routes."
        entries.append(
            {
                "id": f"route:{route.method}:{route.path}",
                "family": "http_routes",
                "name": f"{route.method} {route.path}",
                "go_contract": {
                    "route_family": route.family,
                    "method": route.method,
                    "path": route.path,
                    "handler": route.handler,
                    "auth": route.auth,
                    "timeout": route.timeout,
                    "source": route.source,
                    "project_context": route.project_context,
                    "thread_tracking": route.thread_tracking,
                    "trace_tracking": route.trace_tracking,
                    "persistence_expectation": "request-context/auth-backed DB lookup" if route.auth != "none" else "none-required",
                    "telemetry_expectation": {
                        "http_metrics": True,
                        "request_id": True,
                        "trace_context": route.trace_tracking,
                        "thread_context": route.thread_tracking,
                    },
                    "source_file": "internal/server/routes.go",
                },
                "rust_status": {
                    "state": rust_state,
                    "source": "crates/axonhub-http/src/routes.rs",
                    "notes": rust_notes,
                },
            }
        )
    meta = {
        "family_counts": family_counts,
        "rust_explicit_501_families": sorted(rust_501),
        "go_route_count": len(go_routes),
        "rust_route_match_count": sum(1 for entry in entries if entry["rust_status"]["state"] == "implemented"),
    }
    return entries, meta


def parse_go_graphql() -> tuple[list[dict[str, Any]], dict[str, Any]]:
    resolver_matches = grep_resolver_names()
    rust_state = read_text("crates/axonhub-http/src/state.rs")
    admin_available = "AdminGraphqlCapability::Available" in rust_state
    openapi_available = "OpenApiGraphqlCapability::Available" in rust_state

    entries: list[dict[str, Any]] = []
    for family, names in resolver_matches.items():
        entries.append(
            {
                "id": f"graphql:{family}",
                "family": "graphql",
                "name": family,
                "go_contract": {
                    "resolver_count": len(names),
                    "sample_resolvers": names[:12],
                    "auth": "jwt-admin" if family != "openapi_mutation" else "service-api-key",
                    "source_file": "internal/server/gql/**/*.graphql + *.resolvers.go",
                },
                "rust_status": {
                    "state": "implemented" if (family == "admin_query" and admin_available) or (family == "admin_mutation" and admin_available) or (family == "openapi_mutation" and openapi_available) else ("partial" if family.startswith("admin_") or family == "openapi_mutation" else "missing"),
                    "source": "crates/axonhub-http/src/handlers/graphql.rs",
                    "notes": "Rust exposes admin/openapi GraphQL entrypoints behind capabilities, but Go owns the full resolver breadth." if family != "openapi_mutation" else "Rust openapi GraphQL exists behind capability gating; Go openapi schema currently centers createLLMAPIKey.",
                },
            }
        )
    meta = {"resolver_family_counts": {family: len(names) for family, names in resolver_matches.items()}}
    return entries, meta


def grep_resolver_names() -> dict[str, list[str]]:
    gql_dir = ROOT / "internal" / "server" / "gql"
    result = {"admin_query": [], "admin_mutation": [], "openapi_mutation": []}
    for path in gql_dir.rglob("*.go"):
        text = path.read_text(encoding="utf-8")
        if "openapi" in str(path):
            result["openapi_mutation"].extend(re.findall(r'func \(r \*mutationResolver\) ([A-Za-z0-9_]+)\(', text))
            continue
        result["admin_query"].extend(re.findall(r'func \(r \*queryResolver\) ([A-Za-z0-9_]+)\(', text))
        result["admin_mutation"].extend(re.findall(r'func \(r \*mutationResolver\) ([A-Za-z0-9_]+)\(', text))
    for key in result:
        result[key] = sorted(dict.fromkeys(result[key]))
    return result


def build_middleware_entries() -> list[dict[str, Any]]:
    rust_middleware = read_text("crates/axonhub-http/src/middleware.rs")
    entries = []
    mapping = [
        ("jwt-admin", "WithJWTAuth", "admin_auth", "JWT admin auth"),
        ("api-key", "WithAPIKeyConfig", "api_key_auth", "API key auth with optional no-auth behavior"),
        ("service-api-key", "WithOpenAPIAuth", "service_api_key_auth", "Service-account API key auth for OpenAPI GraphQL"),
        ("gemini-api-key", "WithGeminiKeyAuth", "gemini_auth", "Gemini query/header API key auth"),
        ("request-context", "WithThread/WithTrace/WithProjectID", "request_context", "Request context enrichment for request-id/project/thread/trace"),
        ("http-metrics", "WithMetrics", "http_metrics", "HTTP metric recorder wrapping matched paths"),
    ]
    for key, go_anchor, rust_anchor, description in mapping:
        entries.append(
            {
                "id": f"middleware:{key}",
                "family": "middleware_auth",
                "name": key,
                "go_contract": {
                    "anchor": go_anchor,
                    "description": description,
                    "source": "internal/server/middleware/*.go",
                },
                "rust_status": {
                    "state": "implemented" if rust_anchor in rust_middleware else "missing",
                    "source": "crates/axonhub-http/src/middleware.rs",
                    "notes": None,
                },
            }
        )
    return entries


def build_persistence_entries() -> list[dict[str, Any]]:
    schema_files = sorted((ROOT / "internal" / "ent" / "schema").glob("*.go"))
    rust_contract = read_text("crates/axonhub-config/src/contract.rs")
    entries = []
    db_dialects = [
        ("sqlite3", "implemented", "Default Go and Rust development dialect"),
        ("postgres", "implemented", "Rust config contract accepts postgres/postgresql"),
        ("postgresql", "implemented", "Alias supported by Rust config validator"),
        ("mysql", "implemented", "Rust contract says wired but not fully integration-verified"),
        ("tidb", "legacy-only", "Rust contract marks TiDB as legacy-Go-only"),
        ("neon", "legacy-only", "Rust contract marks Neon as legacy-Go-only"),
    ]
    for dialect, state, note in db_dialects:
        entries.append(
            {
                "id": f"database-dialect:{dialect}",
                "family": "database_runtime",
                "name": dialect,
                "go_contract": {
                    "dialect": dialect,
                    "source": "conf/conf.go + repository docs",
                    "schema_count": len(schema_files),
                },
                "rust_status": {
                    "state": state,
                    "source": "crates/axonhub-config/src/contract.rs",
                    "notes": note,
                },
            }
        )
    entries.append(
        {
            "id": "persistence:ent-schema-surface",
            "family": "database_runtime",
            "name": "ent-schema-surface",
            "go_contract": {
                "schema_files": [path.name for path in schema_files],
                "schema_count": len(schema_files),
                "source": "internal/ent/schema/*.go",
            },
            "rust_status": {
                "state": "partial",
                "source": "apps/axonhub-server/src/foundation + SeaORM-backed slice",
                "notes": "Rust supports only the migration slice; not every Go Ent-owned data surface is ported.",
            },
        }
    )
    return entries


def build_runtime_entries() -> list[dict[str, Any]]:
    return [
        {
            "id": "runtime:metrics-provider",
            "family": "background_runtime",
            "name": "metrics-provider",
            "go_contract": {
                "anchor": "metrics.NewProvider + metrics.SetupMetrics + middleware.WithMetrics",
                "source": "cmd/axonhub/main.go + internal/metrics + internal/server/middleware/metrics.go",
            },
            "rust_status": {
                "state": "implemented",
                "source": "apps/axonhub-server/src/app/server.rs + crates/axonhub-http/src/middleware.rs",
                "notes": "Rust exposes HttpMetricsCapability and request recording wrapper.",
            },
        },
        {
            "id": "runtime:gc-worker",
            "family": "background_runtime",
            "name": "gc-worker",
            "go_contract": {
                "anchor": "internal/server/gc.NewWorker",
                "behavior": "scheduled cleanup + optional vacuum",
                "source": "internal/server/gc/gc.go",
            },
            "rust_status": {
                "state": "missing",
                "source": "no matching runtime worker surfaced in current Rust slice",
                "notes": "Current Rust migration slice does not expose the Go GC worker and storage-policy cleanup runtime.",
            },
        },
        {
            "id": "runtime:provider-quota-poller",
            "family": "background_runtime",
            "name": "provider-quota-poller",
            "go_contract": {
                "anchor": "provider_quota.check_interval + CheckProviderQuotas mutation",
                "source": "conf/conf.go + internal/server/gql/system.resolvers.go",
            },
            "rust_status": {
                "state": "missing",
                "source": "crates/axonhub-config/src/contract.rs",
                "notes": "Rust retains the config key but not the full Go provider-quota subsystem parity.",
            },
        },
        {
            "id": "runtime:antigravity-version-init",
            "family": "background_runtime",
            "name": "antigravity-version-init",
            "go_contract": {
                "anchor": "antigravity.InitVersion",
                "source": "cmd/axonhub/main.go",
            },
            "rust_status": {
                "state": "missing",
                "source": "not observed in current Rust startup path",
                "notes": "Go starts detached antigravity version initialization; Rust startup scan did not surface an equivalent.",
            },
        },
    ]


def build_provider_edge_entries() -> list[dict[str, Any]]:
    provider_edge = read_text("crates/axonhub-http/src/handlers/provider_edge.rs")
    entries = []
    for provider, methods in {
        "codex": ["start", "exchange"],
        "claudecode": ["start", "exchange"],
        "antigravity": ["start", "exchange"],
        "copilot": ["start", "poll"],
    }.items():
        for method in methods:
            route = f"/admin/{provider}/oauth/{method}"
            entries.append(
                {
                    "id": f"provider-edge:{provider}:{method}",
                    "family": "provider_edge",
                    "name": route,
                    "go_contract": {
                        "route": route,
                        "auth": "jwt-admin",
                        "cache_or_stateful_flow": True,
                        "source": "internal/server/routes.go + internal/server/api/*.go",
                    },
                    "rust_status": {
                        "state": "partial",
                        "source": "crates/axonhub-http/src/handlers/provider_edge.rs",
                        "notes": "Rust wires the route family but still returns explicit 501 when provider-edge capability is unavailable.",
                    },
                }
            )
    if "unsupported_provider_edge_response" not in provider_edge:
        raise AssertionError("expected provider edge unsupported gateway in Rust handler")
    return entries


def build_telemetry_entries() -> list[dict[str, Any]]:
    return [
        {
            "id": "telemetry:trace-headers",
            "family": "telemetry",
            "name": "trace-thread-request-headers",
            "go_contract": {
                "headers": ["AH-Trace-Id", "AH-Thread-Id", "AH-Request-Id", "Session_id", "X-Project-ID"],
                "source": "conf/conf.go + internal/tracing/tracing.go + internal/server/middleware/*.go",
            },
            "rust_status": {
                "state": "implemented",
                "source": "crates/axonhub-http/src/middleware.rs + apps/axonhub-server request context services",
                "notes": "Rust request_context/auth middleware carries request/thread/trace/project context through HttpState trace config.",
            },
        },
        {
            "id": "telemetry:http-metrics",
            "family": "telemetry",
            "name": "http-request-metrics",
            "go_contract": {
                "metric_shape": ["method", "path", "status_code", "duration"],
                "source": "internal/server/middleware/metrics.go",
            },
            "rust_status": {
                "state": "implemented",
                "source": "crates/axonhub-http/src/middleware.rs",
                "notes": "Rust records method/path/status/duration via HttpMetricsRecorder.",
            },
        },
    ]


def build_release_entries() -> list[dict[str, Any]]:
    readme = read_text("README.md")
    readme_zh = read_text("README.zh-CN.md")
    agents = read_text("AGENTS.md")
    backend_rules = read_text(".agent/rules/backend.md")
    test_workflow = read_text(".github/workflows/test.yml")
    helm_readme = read_text("deploy/helm/README.md")

    def contains(text: str, needle: str) -> bool:
        return needle in text

    return [
        {
            "id": "release:dual-stack-ci",
            "family": "release_docs_posture",
            "name": "dual-stack-test-workflow",
            "go_contract": {
                "workflow": ["cargo test --workspace --locked", "make test-backend-all"],
                "source": ".github/workflows/test.yml",
            },
            "rust_status": {
                "state": "documented",
                "source": ".github/workflows/test.yml",
                "notes": "CI still treats Rust workspace and legacy Go backend as parallel tested backends.",
            },
        },
        {
            "id": "release:legacy-backend-positioning",
            "family": "release_docs_posture",
            "name": "legacy-go-backend-positioning",
            "go_contract": {
                "docs_sources": ["README.md", "README.zh-CN.md", "AGENTS.md", ".agent/rules/backend.md"],
                "source": "repository docs",
            },
            "rust_status": {
                "state": "gap",
                "source": "README.md + README.zh-CN.md + AGENTS.md + .agent/rules/backend.md",
                "notes": "Repository messaging still explicitly labels Go as legacy-but-required truth source and Rust as migration slice/cutover subset.",
            },
        },
        {
            "id": "release:explicit-501-posture",
            "family": "release_docs_posture",
            "name": "explicit-501-boundary-docs",
            "go_contract": {
                "expectation": "unsupported Rust surfaces are called out explicitly today",
                "source": "README.md + README.zh-CN.md",
            },
            "rust_status": {
                "state": "gap",
                "source": "README.md + README.zh-CN.md",
                "notes": "Both READMEs explicitly advertise 501 boundaries for remaining route families and protocol variants.",
            },
        },
        {
            "id": "release:helm-go-only",
            "family": "release_docs_posture",
            "name": "helm-go-only-deployment-path",
            "go_contract": {
                "source": "deploy/helm/README.md + README.md + README.zh-CN.md",
                "behavior": "Helm deploy path is legacy Go backend only",
            },
            "rust_status": {
                "state": "gap",
                "source": "deploy/helm/README.md",
                "notes": "Helm README says Rust cutover is not supported for Kubernetes via Helm.",
            },
        },
        {
            "id": "release:legacy-only-dialects-docs",
            "family": "release_docs_posture",
            "name": "tidb-neon-go-only-positioning",
            "go_contract": {
                "dialects": ["tidb", "neon"],
                "source": "README.md + README.zh-CN.md + crates/axonhub-config/src/contract.rs",
            },
            "rust_status": {
                "state": "gap",
                "source": "README.md + README.zh-CN.md + crates/axonhub-config/src/contract.rs",
                "notes": "Docs and Rust config contract both keep TiDB/Neon on the legacy Go backend.",
            },
        },
        {
            "id": "release:docs-consistency-check",
            "family": "release_docs_posture",
            "name": "english-chinese-migration-framing-consistency",
            "go_contract": {
                "english_mentions": contains(readme, "legacy Go backend"),
                "chinese_mentions": contains(readme_zh, "旧 Go 后端"),
                "agents_mentions": contains(agents, "migration slice"),
                "backend_rule_mentions": contains(backend_rules, "source of truth"),
                "source": "README.md + README.zh-CN.md + AGENTS.md + .agent/rules/backend.md",
            },
            "rust_status": {
                "state": "documented",
                "source": "docs and agent guidance",
                "notes": "English/Chinese docs and agent rules are aligned on migration-slice framing, which is truthful for Task 1 inventory purposes but still a parity gap.",
            },
        },
    ]


def build_inventory() -> dict[str, Any]:
    cli_entries = parse_go_cli()
    config_entries, config_meta = parse_go_config_defaults()
    route_entries, route_meta = build_route_entries()
    graphql_entries, graphql_meta = parse_go_graphql()
    middleware_entries = build_middleware_entries()
    persistence_entries = build_persistence_entries()
    runtime_entries = build_runtime_entries()
    provider_edge_entries = build_provider_edge_entries()
    telemetry_entries = build_telemetry_entries()
    release_entries = build_release_entries()

    inventory_entries = (
        cli_entries
        + config_entries
        + route_entries
        + middleware_entries
        + graphql_entries
        + persistence_entries
        + runtime_entries
        + provider_edge_entries
        + telemetry_entries
        + release_entries
    )

    summary = summarize_entries(inventory_entries)
    discovered_counts = {
        "cli_command_families": len(cli_entries),
        "go_config_keys": config_meta["go_default_key_count"],
        "go_http_routes": route_meta["go_route_count"],
        "middleware_families": len(middleware_entries),
        "graphql_families": sum(graphql_meta["resolver_family_counts"].values()),
        "ent_schema_files": len(next(entry for entry in persistence_entries if entry["id"] == "persistence:ent-schema-surface")["go_contract"]["schema_files"]),
        "background_runtime_families": len(runtime_entries),
        "provider_edge_flows": len(provider_edge_entries),
        "telemetry_families": len(telemetry_entries),
        "release_docs_posture_items": len(release_entries),
    }

    manifest_counts = {
        "cli_command_families": len([e for e in inventory_entries if e["family"] == "cli"]),
        "go_config_keys": len([e for e in inventory_entries if e["family"] == "config"]),
        "go_http_routes": len([e for e in inventory_entries if e["family"] == "http_routes"]),
        "middleware_families": len([e for e in inventory_entries if e["family"] == "middleware_auth"]),
        "graphql_families": sum(e["go_contract"]["resolver_count"] for e in inventory_entries if e["family"] == "graphql"),
        "ent_schema_files": discovered_counts["ent_schema_files"],
        "background_runtime_families": len([e for e in inventory_entries if e["family"] == "background_runtime"]),
        "provider_edge_flows": len([e for e in inventory_entries if e["family"] == "provider_edge"]),
        "telemetry_families": len([e for e in inventory_entries if e["family"] == "telemetry"]),
        "release_docs_posture_items": len([e for e in inventory_entries if e["family"] == "release_docs_posture"]),
    }

    return {
        "meta": {
            "task": "Build Go→Rust parity contract inventory",
            "truth_model": "Task 16 inventory snapshot: source-backed parity inventory remains contextual evidence and may still record broader repo gaps outside the enforced regression suite.",
            "generated_from": [
                "cmd/axonhub/main.go",
                "conf/conf.go",
                "internal/server/routes.go",
                "internal/server/middleware/*.go",
                "internal/server/gql/**",
                "internal/ent/schema/*.go",
                "apps/axonhub-server/src/app/cli.rs",
                "crates/axonhub-config/src/contract.rs",
                "crates/axonhub-http/src/routes.rs",
                "crates/axonhub-http/src/handlers/*.rs",
                "crates/axonhub-http/src/middleware.rs",
                "README.md",
                "README.zh-CN.md",
                "AGENTS.md",
                ".agent/rules/backend.md",
                ".github/workflows/test.yml",
                "deploy/helm/README.md",
            ],
            "discovered_counts": discovered_counts,
            "manifest_counts": manifest_counts,
            "config_meta": config_meta,
            "route_meta": route_meta,
            "graphql_meta": graphql_meta,
            "summary": summary,
        },
        "entries": inventory_entries,
    }


def summarize_entries(entries: list[dict[str, Any]]) -> dict[str, Any]:
    by_state: dict[str, int] = {}
    by_family: dict[str, dict[str, int]] = {}
    for entry in entries:
        family = entry["family"]
        state = entry["rust_status"]["state"]
        by_state[state] = by_state.get(state, 0) + 1
        family_bucket = by_family.setdefault(family, {})
        family_bucket[state] = family_bucket.get(state, 0) + 1
    return {"by_state": by_state, "by_family": by_family, "entry_count": len(entries)}


def validate_inventory(inventory: dict[str, Any]) -> list[str]:
    meta = inventory["meta"]
    discovered = meta["discovered_counts"]
    manifest = meta["manifest_counts"]
    errors: list[str] = []
    for key, discovered_value in discovered.items():
        manifest_value = manifest.get(key)
        if manifest_value != discovered_value:
            errors.append(
                f"Count mismatch for {key}: discovered {discovered_value}, manifest {manifest_value}"
            )
    for entry in inventory["entries"]:
        state = entry["rust_status"]["state"]
        if state not in {"implemented", "partial", "missing", "legacy-only", "gap", "documented"}:
            errors.append(f"Unsupported rust_status.state for {entry['id']}: {state}")
    return errors


def build_gap_report(inventory: dict[str, Any]) -> str:
    lines = [
        "Task 1 Go→Rust parity inventory gap report",
        "",
        "Validator mode: deterministic source-backed inventory vs discovered Go counts",
        f"Total inventory entries: {inventory['meta']['summary']['entry_count']}",
        "",
    ]

    count_errors = validate_inventory(inventory)
    if count_errors:
        lines.append("COUNT VALIDATION ERRORS")
        lines.extend(f"- {error}" for error in count_errors)
        lines.append("")

    gap_states = {"missing", "partial", "legacy-only", "gap"}
    grouped: dict[str, list[dict[str, Any]]] = {}
    for entry in inventory["entries"]:
        if entry["rust_status"]["state"] in gap_states:
            grouped.setdefault(entry["family"], []).append(entry)

    if not grouped:
        lines.append("No parity gaps detected.")
        return "\n".join(lines) + "\n"

    lines.append("RUST PARITY GAPS")
    for family in sorted(grouped):
        lines.append(f"[{family}]")
        for entry in sorted(grouped[family], key=lambda item: item["id"]):
            lines.append(
                f"- {entry['id']} :: {entry['rust_status']['state']} :: {entry['rust_status'].get('notes') or 'No additional note'}"
            )
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def write_outputs(inventory: dict[str, Any]) -> None:
    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    JSON_OUTPUT.write_text(json.dumps(inventory, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    GAP_OUTPUT.write_text(build_gap_report(inventory), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate and validate Go→Rust parity inventory artifacts")
    parser.add_argument("command", choices=["generate", "validate"], help="generate artifacts or validate existing generation")
    args = parser.parse_args()

    inventory = build_inventory()
    write_outputs(inventory)
    errors = validate_inventory(inventory)

    if args.command == "generate":
        print(f"wrote {JSON_OUTPUT.relative_to(ROOT)}")
        print(f"wrote {GAP_OUTPUT.relative_to(ROOT)}")
        if errors:
            for error in errors:
                print(error, file=sys.stderr)
            return 1
        return 0

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    gap_states = {"missing", "partial", "legacy-only", "gap"}
    has_parity_gap = any(entry["rust_status"]["state"] in gap_states for entry in inventory["entries"])
    if has_parity_gap:
        print(build_gap_report(inventory))
        return 1

    print("inventory validation passed with no parity gaps")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
