# Host sheets — `hosts/*.toml`

One file per host. The common part of the schema is `bbl-hostrules` (`id`, `name`, `vendor`,
`kind`, `status`, `application`, `references`, `hints`, `locations[]` with `scope`, `format`,
`path`, `legacy`, `shared_with`, `source`). Billfold adds what an inventory of accounts, keys
and usage needs. A location without a sourced quote is refused at load; a host with neither
a location nor a reference is refused.

## Host-level fields

| Field | Meaning |
|---|---|
| `detect.paths`, `detect.binaries` | How the host is deemed present: a path (glob allowed) that exists, or a binary on `PATH`. |
| `keyring.service`, `keyring.source` | Where the host keeps secrets when not in a file. Billfold never opens the keyring; the service name is shown and sourced. |
| `usage_url` | The host's own page where usage is shown, when it has one (an `open-url` action, never fetched). |
| `no_usage_log` | For a host that keeps no per-request token log: the reason, shown as the `no-usage-log` finding. |
| `check` | The host's own commands to check an account or usage (`claude`, then `/status`), shown and copied, never run. |

## Location fields (`kind` is required)

| Field | Meaning |
|---|---|
| `kind` | `credential` (a login store), `settings` (names the provider, the model, sometimes keys), `usage` (a per-request token log). |
| `credit` | For a credential: `subscription`, `api-key`, `mixed` (a field decides), `keyring-fallback` (the file is the fallback of the keyring). |
| `provider` | The provider this credential is for (`anthropic`, `openai`, `github`…). |
| `fields.plan`, `.email`, `.organization`, `.expires` (+ `.expires_unit` = `ms` or `s`) | Key paths of the account-descriptive fields that are safe to show. Only the e-mail's domain is kept. |
| `fields.mode`, `.mode_subscription`, `.mode_api_key` | A field whose values tell a subscription from a key (`auth_mode`, `security.auth.selectedType`). |
| `fields.jwt`, `fields.jwt_claims.{plan,email,account,expires}` | A JWT whose payload is decoded locally (never verified, never shown) for its claims. Dots in a claim name are escaped `\.`. |
| `fields.api_key` | A key path whose presence means an API key is stored. |
| `fields.provider`, `fields.model` | For a settings file: the default provider and model. |
| `fields.providers_table` | A table whose children are providers (`""` = the root; OpenCode's SQLite `credential` table). |
| `secrets` | Key paths that are secrets — never shown; wildcards allowed. |
| `keys_in` | Key paths whose children are provider keys in clear (`env`, `mcpServers.*.env`, `""` for a whole secrets file), scanned by `keys`. |
| `sqlite_kv.{table,key_column,value_column}` | For a `sqlite` credential that is a key/value table (VS Code's `ItemTable`). |
| `usage.reader` | One of `claude-jsonl`, `codex-jsonl`, `gemini-jsonl`, `qwen-jsonl`, `opencode-sqlite`, `goose-sqlite`, `aider-markdown`, `cline-tasks`, `cline-sessions`, `kilo-sqlite`, `copilot-jsonl`, `zed-sqlite`, `amp-json`. |
| `usage.retention_days`, `.retention_setting`, `.retention_file` | The host's default retention and the setting that changes it (`cleanupPeriodDays` in `~/.claude/settings.json`, `"30d"` strings accepted). |
| `usage.cost_by_host`, `usage.quota` | The host writes its own cost per request; the host writes its remaining quota. |
| `note` | What the reader must know that the fields cannot say. |

## Source

Every `source` has `kind` (`documentation`, `source-code`, `observed`), `url`, `read_on`,
`quote` (the words the rule was read from) and an optional `note` (file and line in a
repository, the commit, a divergence between the docs and the code). `observed` marks a
third-party reading (a tool's source, a forum) when the host documents nothing — it is said
in the finding.
