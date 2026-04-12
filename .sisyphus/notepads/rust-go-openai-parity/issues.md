# Issues

- Live HTTP QA for this slice was blocked because the local admin server was not listening on `127.0.0.1:8090` during verification.
- The broad `cargo test -p axonhub-server` run remains noisy with many pre-existing warnings, so slice-level regressions are easiest to spot via targeted tests plus LSP diagnostics.
- `requestStatsByModel` needed a dedicated coverage pass because it is not yet wired into the older Go-oriented notepad examples, so parity had to be inferred from the Go resolver plus the existing channel path.
## 2026-04-12 - requestStatsByAPIKey parity

- Query shape needed a second pass: grouping by `api_key_id` is straightforward, but API key labels must be fetched separately to avoid mixing in deleted rows.
- Targeted cargo tests passed; live HTTP QA still needs a direct authenticated admin request against the running server.
- Live HTTP QA against the pre-existing local 8090 service initially hit an older binary that still returned GraphQL 501 for tokenStats; final verification used rebuilt local artifacts plus in-process test coverage, while standalone temp-config boot still loaded repo-root config.yml before HOME config.
