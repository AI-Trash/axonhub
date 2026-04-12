# Learnings

- The active `/admin/graphql` subset can stay small by branching on `first_graphql_field_name(query)` and reusing the shared SeaORM request ledger for dashboard-style aggregates.
- For `requestStatsByChannel`, the Go semantics map cleanly to `usage_logs` joined with `channels`, with deleted channels filtered out and results sorted by count desc.
- A tiny `timeWindow` parser that only accepts `day`, `week`, `month`, and `allTime` is enough to keep the Rust subset truthful without silently returning wrong data.
- Live QA confirmed the active Rust `/admin/graphql` route now returns `requestStatsByChannel(timeWindow: "allTime")` with grouped channel counts on the real HTTP path.
- `requestStatsByModel` can reuse the same `parse_admin_graphql_time_window` helper and the same `read_dashboard` gate as channel stats, while grouping directly on `usage_logs.model_id`.
- Broad foundation tests already cover the new model slice once the request ledger rows are inserted manually alongside `requests`.
- Live QA confirmed the active Rust `/admin/graphql` route now returns `requestStatsByModel(timeWindow: "allTime")` with grouped model counts on the real HTTP path.
- The next smallest dashboard slice is `tokenStats`: unlike channel/model/API-key breakdowns, it is a single aggregate object over `usage_logs` and already has a frontend consumer on first dashboard load.
- Live QA confirmed the active Rust `/admin/graphql` route now returns `tokenStats` with aggregate token counters and a truthful `lastUpdated` string.
## 2026-04-12 - requestStatsByAPIKey parity

- Admin GraphQL active subset can stay truthful by using `usage_logs` grouped on `api_key_id`, then hydrating API key names from the active API key table.
- `timeWindow` parsing already exists; reuse it and keep `day|week|month|allTime` semantics identical across request stats slices.
- tokenStats parity fits the active SeaORM /admin/graphql dispatcher by reusing the existing usage_logs aggregation shape and returning lastUpdated from MAX(created_at) normalized to RFC3339.
- Live QA confirmed the active Rust `/admin/graphql` route now returns `requestStatsByAPIKey(timeWindow: "allTime")` with hydrated API key names and grouped counts on the real HTTP path.

## 2026-04-12 - cost stats parity

- `costStatsByModel` is the smallest truthful follow-up to `tokenStats` because it reuses the existing `timeWindow` parser and groups directly on `usage_logs.model_id` with a single `SUM(total_cost)` aggregate.
- `costStatsByChannel` can stay on the active SeaORM subset by mirroring `requestStatsByChannel`: join `usage_logs` to `channels`, filter deleted channels, and return `channelName` so the frontend can merge requests and cost data by name.
- SeaORM `FromQueryResult` structs require the selected column aliases to match the Rust field names; omitting `channel_name` broke the first `costStatsByChannel` attempt even though the SQL aggregate itself was correct.
- `channelSuccessRates` is a better next dashboard usability slice than deeper analytics because the card is rendered in the always-visible overview section, not behind a collapsed panel.
- The Go semantics map cleanly to the active Rust path by aggregating `request_executions` on non-null `channel_id`, counting `completed` and `failed` rows only, then hydrating channel name/type from the channels table.
- The Rust subset can preserve truthful Go parity without extra filters by leaving deleted-channel behavior aligned with Go’s channel hydration step and keeping the top-5 ordering on total execution count.
- `costStatsByAPIKey` is the smallest truthful follow-up after `requestStatsByAPIKey` because it reuses the same API-key hydration pattern and only swaps `COUNT(*)` for `SUM(total_cost)`.
- The active API-key requests/cost chart joins on `apiKeyName`, so the Rust subset must preserve hydrated names for non-deleted API keys rather than returning raw IDs only.
- For cost fixtures, decimal choices matter in Rust tests: binary-safe sums like `0.5 + 0.75` avoid false negatives that come from floating-point display noise such as `1.2999999999999998`.
- `tokenStatsByAPIKey` can stay on the active SeaORM subset by aggregating directly from `usage_logs.api_key_id`; unlike the legacy Go resolver, Rust already stores `api_key_id` on the usage row and does not need to join through `requests`.
- The frontend only renders input, output, cached, and total token bars, but parity still needs `reasoningTokens` in the payload because the GraphQL contract includes it and `totalTokens` is computed from all four buckets.
- Go trims `tokenStatsByAPIKey` to the top 3 rows rather than 10, so the Rust parity slice should preserve that behavior even though the frontend chart can tolerate fewer than 10 entries.
- `tokenStatsByChannel` can reuse the same active-channel join as `requestStatsByChannel` and `costStatsByChannel`; the only new logic is summing the four token buckets and computing `totalTokens` in Rust.
- The channel token slice should sort and truncate after computing the full total (`input + output + cached + reasoning`) so the returned top rows reflect the actual contract field, not just `usage_logs.total_tokens` or DB ordering quirks.
- Deleted-channel filtering remains important for token charts too: the chart would otherwise surface historical channels that the current admin UI intentionally suppresses in the channel analytics family.
- `dailyRequestStats` is the first remaining always-visible dashboard blocker after the token/cost charts because the overview mounts it on initial page load rather than behind a collapsed section.
- Truthful parity for `dailyRequestStats` requires named timezone handling from `system_general_settings.timezone`; a UTC-only approximation would silently misbucket dates for operators outside UTC.
- The smallest Rust implementation path is a bounded 30-day in-memory aggregation after filtering `usage_logs` by the UTC start of the local window; that keeps the slice honest without widening into hourly/performance SQL complexity.
- `tokenStatsByModel` is cheaper than the channel/API-key token slices because it stays on a single `usage_logs` table and only groups by `model_id`, with no joins or hydration lookups.
- The model section on the dashboard was already half-working: `requestStatsByModel` and `costStatsByModel` were live, so `tokenStatsByModel` was the single missing card in that dimension.
- Adding one explicit time-window-filter test for `tokenStatsByModel` is worthwhile even when sibling token slices skipped it, because this field’s contract is simple and the extra coverage cheaply locks down the `month/day/week` filter path.
- `fastestChannels` is not a naive top-throughput aggregate: Go dedupes to the latest completed execution per request, ignores zero/absent latency rows, and only then joins usage logs to compute throughput.
- The Go confidence sorter does not drop all low-confidence rows; it only filters to medium/high when there are already enough of them to satisfy the requested limit. Otherwise, low-confidence rows remain in the result.
- For this slice, invalid `timeWindow` values default to `day` in Go, which is different from the stricter error behavior used by the request/token/cost chart slices. The Rust parity implementation should preserve that difference rather than force one global rule.
- `fastestModels` truly is the model-side sibling of `fastestChannels`: same input shape, same latest-completed dedupe, same confidence ranking, but grouped by the request model instead of channel metadata.
- The model name for `fastestModels` comes from model metadata when present, but falling back to `modelId` keeps the Rust active path resilient when the models table is incomplete while still satisfying the frontend contract.
- Reusing the `FastestChannelsInput` object type for `fastestModels` matches the Go/frontend contract exactly and avoids inventing a parallel input shape just for the Rust subset path.
- `modelPerformanceStats` is the first remaining slice where grouping by local day matters more than a simple time-window filter; reusing the timezone helpers from `dailyRequestStats` kept the active Rust path truthful without widening into the Go raw-SQL builders.
- Go’s `modelPerformanceStats` does not return every model seen in the 30-day window; it first aggregates daily rows, then selects the top performers by total request volume across the full period, and only returns those models’ rows.
- The daily model performance path must bucket by `request_executions.created_at`, not `usage_logs.created_at`; the failing timezone test exposed that difference because the execution timestamp is what Go uses for daily performance semantics.
- `channelPerformanceStats` is the first remaining dashboard slice that intentionally mixes two data sources: Go prefers `channel_probes` when probe rows exist and only falls back to execution-derived metrics when probes are absent.
- For probe-derived channel performance, throughput and TTFT are weighted by `total_request_count`, not averaged naively per row. Preserving that weighting keeps sparse probes from distorting the chart.
- The active Rust path can stay truthful without importing Go’s raw SQL builder by reusing the existing timezone helpers plus a probe-first / execution-fallback implementation directly in `graphql.rs`; targeted tests were enough to lock down both branches.
- `channelProbeData` was not missing backend logic; the Rust async-graphql field and operational loader already existed. The real parity gap was only that the active SeaORM subset dispatcher never routed the query.
- The `channelProbeData` frontend contract is one of the few places where the exact field name `channelID` still matters; returning the more common `channelId` shape would silently break the channels page probe map.
- Probe timestamps must be seeded on the loader’s normalized interval grid in tests. Inserting an arbitrary Unix second can produce the right data in storage but still miss every returned normalized bucket.
- `systemGeneralSettings` was another partial-support trap: Rust already read the timezone out of `system_general_settings` for analytics, but the active `/admin/graphql` subset still lacked both the public query and the mutation.
- The smallest truthful implementation is to reuse the generic `systems` JSON helpers in the subset repository, then add field-by-field default fill on top of deserialization instead of introducing a new settings storage layer.
- Preserving partial-mutation merge behavior matters even if the current frontend submits both fields together, because the GraphQL input contract makes both fields optional and Go does not wipe omitted values.
- `apiKeyTokenUsageStats` is the first remaining slice where the active frontend contract uses GraphQL-style acronyms in input fields (`createdAtGTE` / `createdAtLTE`) that do not round-trip from a naive Rust `camelCase` rename; they need explicit serde/graphql renames.
- The existing API-key token aggregate helpers in Rust covered most of the math already, so the real parity work was input validation, date filtering, and shaping the nested `topModels` response exactly like Go.
- When a targeted test suddenly aggregates both old and recent rows, it’s often a contract-name deserialization bug rather than broken filter SQL; the failed range test exposed that immediately.
