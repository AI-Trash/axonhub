# Problems

- The current unit/integration harness proves multiple HTTP body chunks are yielded in order, but it still does not measure wall-clock flush timing across a real socket boundary; keep that limitation in mind for future parity QA.
- Live socket QA now proves chunked event-stream delivery from AxonHub with a real SSE upstream, but local socket reads can still coalesce multiple upstream flushes; this is good enough to verify streaming transport is real, not to benchmark flush latency precisely.
- Broader parity beyond the prioritized OpenAI/Codex/runtime slices still remains, especially outside the narrowed workstream (for example stale image-generation docs and larger admin GraphQL surface differences).

- The repository's broad Rust test run produces heavy warning noise, so focused test names are the most practical way to confirm small parity slices quickly.
- Live HTTP QA for this slice was blocked because the managed local server was not listening on `127.0.0.1:8090` during verification, so only test-suite coverage was available in-session.
- The active SeaORM GraphQL dispatcher still depends on ad-hoc string parsing for scalar arguments like `level`, so more complex GraphQL inputs will need a safer parser if future parity slices expand there.
- The new model stats slice is simple, but it still inherits the same top-10 truncation/sort behavior as the Go resolver, so future analytics slices should watch for silent ordering assumptions.
