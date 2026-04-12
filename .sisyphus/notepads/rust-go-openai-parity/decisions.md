# Decisions

- Keep `requestStatsByChannel` on the active SeaORM subset dispatcher path instead of adding a new async-graphql helper surface.
- Reject unsupported `timeWindow` values with a clear GraphQL error rather than defaulting to an unsafe fallback.
- Scope the query to `read_dashboard` and keep the response shape limited to `channelName` and `count`.
- Continue dashboard parity by adding one top-level grouped metric at a time rather than attempting the full analytics surface in one pass.
- Keep `requestStatsByModel` on the same SeaORM subset dispatcher branch as `requestStatsByChannel` so the admin GraphQL surface stays consistent.
- Continue filling the dashboard analytics surface one top-level grouped metric at a time, starting with the least stateful counters before token/cost/performance aggregations.
- Prefer `tokenStats` before cost/performance/top-project charts because it is still first-load visible UI, but requires only one aggregate payload instead of multiple grouped result sets.
- Keep `tokenStats` as a single aggregate object over `usage_logs` and avoid forcing RFC3339 normalization for `lastUpdated` until the product requires it; the frontend only needs a string-or-null.
## 2026-04-12 - requestStatsByAPIKey parity

- Chose a two-step SeaORM query: aggregate counts from `usage_logs`, then resolve API key names from `api_keys` by ID. This keeps the route on the active SeaORM subset dispatcher path while preserving truthful names.
- Kept tokenStats Rust slice scoped to direct usage_logs sums over prompt_tokens, completion_tokens, and prompt_cached_tokens with UTC calendar windows matching the existing Rust dashboard counting pattern and read_dashboard gate.
- Keep `requestStatsByAPIKey` on the same active SeaORM subset family as the channel/model slices; it is still a low-risk grouped-count query even though it needs one extra name lookup.

## 2026-04-12 - cost stats parity

- Keep `costStatsByModel` and `costStatsByChannel` on the active SeaORM `/admin/graphql` dispatcher instead of widening the alternate async-graphql surface.
- Preserve the frontend join keys exactly: `modelId` for model charts and `channelName` for channel charts, so the requests/cost composite cards can reuse existing client-side merge logic.
- For channel cost, follow Go’s deleted-channel behavior and sort by total cost descending with a deterministic name tiebreaker before truncating to the top 10 rows.
- Keep `channelSuccessRates` on the active SeaORM subset dispatcher and scope it to `read_dashboard`; it is an operator-facing overview metric, not a secondary analytics chart.
- Match Go’s contract shape exactly for `channelSuccessRates`: `channelId`, `channelName`, `channelType`, `successCount`, `failedCount`, `totalCount`, and `successRate`.
- Use top-5 ordering by total execution count for `channelSuccessRates`, with deterministic channel-id tiebreaking before hydrating channel metadata.
- Keep `costStatsByAPIKey` on the active SeaORM `/admin/graphql` dispatcher and implement it as the cost-side sibling of `requestStatsByAPIKey`, not as part of a broader token/billing refactor.
- Preserve the Go/frontend contract shape exactly for `costStatsByAPIKey`: `apiKeyId`, `apiKeyName`, and `cost`, with top-10 ordering by total cost descending.
- Reuse the deleted-API-key filtering already present in the request-by-API-key Rust slice so the requests/cost composite chart stays internally consistent.
- Keep `tokenStatsByAPIKey` on the active SeaORM `/admin/graphql` dispatcher and implement it as the token-side sibling of the existing API-key request/cost slices.
- Preserve the full Go/frontend contract shape for `tokenStatsByAPIKey`: `apiKeyId`, `apiKeyName`, `inputTokens`, `outputTokens`, `cachedTokens`, `reasoningTokens`, and `totalTokens`.
- Preserve Go’s top-3 ordering by computed total tokens for `tokenStatsByAPIKey`, with deterministic `api_key_id` tiebreaking and the same deleted-API-key name hydration filter used by the other API-key slices.
- Keep `tokenStatsByChannel` on the active SeaORM `/admin/graphql` dispatcher and implement it as the token-side sibling of the existing channel request/cost slices.
- Preserve the Go/frontend contract shape for `tokenStatsByChannel`: `channelName`, `inputTokens`, `outputTokens`, `cachedTokens`, `reasoningTokens`, and `totalTokens`.
- Preserve deleted-channel filtering and top-10 ordering by computed total tokens for `tokenStatsByChannel`, with deterministic `channel_name` tiebreaking after aggregation.
- Keep `dailyRequestStats` on the active SeaORM `/admin/graphql` dispatcher and scope it to `read_dashboard`; it is part of the always-visible overview, not an optional analytics panel.
- Preserve the Go/frontend contract shape for `dailyRequestStats`: 30 ascending daily points with `date`, `count`, `tokens`, and `cost`, including zero-filled missing days.
- Use `system_general_settings.timezone` with UTC fallback for invalid or missing values so local-date bucketing matches the legacy Go behavior and the frontend chart labels remain truthful.
- Keep `tokenStatsByModel` on the active SeaORM `/admin/graphql` dispatcher as the model-side sibling of `requestStatsByModel` and `costStatsByModel`.
- Preserve the Go/frontend contract shape for `tokenStatsByModel`: `modelId`, `inputTokens`, `outputTokens`, `cachedTokens`, `reasoningTokens`, and `totalTokens`.
- Sort `tokenStatsByModel` by computed displayed total tokens descending, with deterministic `model_id` tiebreaking, and keep the top-10 truncation on the active Rust path.
- Keep `fastestChannels` on the active SeaORM `/admin/graphql` dispatcher and implement it with structured `input` parsing rather than trying to shoehorn it into the scalar-argument helpers used by the chart aggregates.
- Preserve the Go/frontend contract shape for `fastestChannels`: `channelId`, `channelName`, `channelType`, `throughput`, `tokensCount`, `latencyMs`, `requestCount`, and `confidenceLevel`.
- Preserve Go-specific behavior for this slice: latest completed execution per request wins, low-confidence rows are only filtered out when enough medium/high rows remain, and invalid `timeWindow` values default to `day`.
- Keep `fastestModels` on the active SeaORM `/admin/graphql` dispatcher as the model-side sibling of `fastestChannels`, reusing the same input object and confidence logic.
- Preserve the Go/frontend contract shape for `fastestModels`: `modelId`, `modelName`, `throughput`, `tokensCount`, `latencyMs`, `requestCount`, and `confidenceLevel`.
- Preserve Go-specific behavior for this slice too: latest completed execution per request wins, low-confidence rows are only filtered out when enough medium/high rows remain, and invalid `timeWindow` values default to `day`.
- Keep `modelPerformanceStats` on the active SeaORM `/admin/graphql` dispatcher and implement it as a dedicated daily-series path rather than trying to force it through the existing fastest-performer helpers.
- Preserve the Go/frontend contract shape for `modelPerformanceStats`: `date`, `modelId`, `throughput`, `ttftMs`, and `requestCount`.
- Preserve Go-specific daily semantics for this slice: 30-day local-date bucketing using `system_general_settings.timezone`, latest completed execution per request, positive-throughput rows only, and top-6 model selection by total request volume after aggregation.
- Keep `channelPerformanceStats` on the active SeaORM `/admin/graphql` dispatcher as a dedicated probe-first daily-series path, with explicit execution fallback when no probes exist.
- Preserve the Go/frontend contract shape for `channelPerformanceStats`: `date`, `channelId`, `channelName`, `throughput`, `ttftMs`, and `requestCount`.
- Preserve Go-specific daily semantics for this slice: 30-day local-date bucketing using `system_general_settings.timezone`, top-6 channel selection by total request volume, weighted probe aggregation when probes exist, and latest-completed execution fallback when they do not.
- Keep `channelProbeData` on the active SeaORM `/admin/graphql` dispatcher as a small wiring slice that reuses the existing operational loader instead of re-implementing probe window logic.
- Preserve the Go/frontend contract shape exactly for `channelProbeData`: top-level `channelID` and nested probe point fields `timestamp`, `totalRequestCount`, `successRequestCount`, `avgTokensPerSecond`, and `avgTimeToFirstTokenMs`.
- Preserve channel read authorization and the existing loader’s normalized bucket behavior; the active-route work should only bridge dispatcher input/output, not fork the loader semantics.
- Keep `systemGeneralSettings` and `updateSystemGeneralSettings` on the active SeaORM `/admin/graphql` dispatcher as a single settings slice because they share the same backing JSON record and permission domain.
- Preserve the Go/frontend contract shape exactly for this slice: query returns `currencyCode` and `timezone`, mutation returns `Boolean!`.
- Preserve Go/default behavior for missing or partial records: read fills absent fields with `USD` / `UTC`, and mutation merges unspecified fields instead of clearing them.
- Keep `apiKeyTokenUsageStats` on the active SeaORM `/admin/graphql` dispatcher as a standalone API-key analytics slice rather than bundling it with the leftover dashboard-only queries.
- Preserve the Go/frontend contract shape exactly: `apiKeyId`, aggregate token counts, and nested `topModels { modelId, inputTokens, outputTokens, cachedTokens, reasoningTokens }`.
- Preserve Go-specific validation and shaping behavior: require non-empty `apiKeyIds`, cap at 100 ids, reject wrong-resource gids with field-level GraphQL errors, apply `createdAtGTE` / `createdAtLTE` to both aggregate and nested queries, and limit `topModels` to the top 3 per API key.
