# Changelog

## [0.1.0] — 2026-10-17

First release: the accounts a machine is wired to, the keys it holds, the tokens it burned —
read-only, no network, from sourced sheets.

### Findings and their place on the four common severities

- **Act now**: `tracked-secret` (a key in clear in a git-tracked file), `exposed-secret` (a
  key in clear other users can read — the file's mode and every directory above it).
- **Review**: `clear-secret`, `shared-key` (same session fingerprint in several places),
  `expired`, `double-credit` (a subscription on the host and a key of the same provider in
  its shell environment), `unpriced` (a model the embedded price table does not know).
- **Good to know**: `pruning-soon` (within 7 days of the host's retention), `host-disagrees`
  (the host's own cost differs by more than 10 %), `near-limit` (Codex's last written quota
  at 80 % or more), `legacy-store`.
- **Information**: `no-usage-log`, `in-keyring`.

### Read

- 21 host sheets (`hosts/*.toml`, schema `docs/hosts.md`): credential stores (JSON, YAML,
  TOML, dotenv, SQLite key/value tables, OpenCode's `credential` table), settings, usage
  logs with their reader, retention and whether the host writes a cost or a quota.
- 12 usage readers: Claude Code JSONL (dedup on message id and request id, sidechain replays
  dropped), Codex rollouts (`.jsonl` and `.zst`, cumulative → delta, rate limits), Gemini
  CLI (last record per id), Qwen Code (auth type in the log), OpenCode and Kilo SQLite (fork
  rows skipped), Goose `usage_ledger`, Aider's Markdown history (rounded counts), Cline and
  Roo task files, Cline session manifests, Copilot CLI session events (cumulative, cache
  buckets inside `inputTokens`), Zed threads (JSON or zstd), Amp threads.
- 21 provider sheets (`providers/*.toml`, schema `docs/providers.md`): documented variable
  names, observed key shapes, usage pages, cache-write multipliers, long-context tiers,
  DeepSeek peak hours, announced changes.
- Keys: process environment, shell start-up files, `environment.d`, project `.env*` and
  `.envrc`, the hosts' own files, MCP configs, `~/.netrc`, `~/.aws/credentials`,
  `~/.cache/huggingface/token`, `llm` and Continue configs. Git state read from `.git/index`
  and `.gitignore` without running git.
- Prices: an embedded snapshot of API Price Index `all.json` (schema 1, with `tier`, `api`,
  base prices and discounts), joined on the API id, its aliases, then the base model.

### Summary figures for the tower

`keys`, `tokens_30d`, `usd_30d`, `findings` (the tower takes four; the provider count is a
note).
