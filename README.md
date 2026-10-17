# Billfold

**What this machine spends on AI — the accounts it is wired to, the keys it holds, the
tokens it burned.**

A developer or a vibe coder accumulates providers (Anthropic, OpenAI, Google, DeepSeek,
Mistral, OpenRouter…), accounts (a Claude Max plan, a ChatGPT plan, an API key here and
there) and tools that spend them (Claude Code, Codex, Cursor, Gemini CLI, OpenCode, Aider,
Cline…). Three questions have no simple answer on the machine itself: **what am I wired
to**, **where are my keys and who can see them**, **how much did I consume**. Each host
stores its login its own way — JSON in clear, SQLite, the system keyring —, keys end up in
`.bashrc`, in a `.env` that git tracks, in a tool's `secrets.json`; and eleven hosts write
their token usage to disk in eleven formats, with their own duplicates, then prune it after
thirty days.

Billfold reads all of that in place and shows it in three views — **Wiring**, **Keys**,
**Meter** — with the findings that matter: a key in a git-tracked file, the same key in
three tools, a subscription and an API key paying twice, a model the price table does not
know, a log about to be pruned. Read-only, no network, secrets masked the moment they are
read, every rule sourced from the host's or the provider's documentation. Part of the
[Blackberrylabs](https://github.com/blackberrylabs) family; the
[control tower](https://github.com/blackberrylabs/blackberrylabs) reads its report.

Tokens are the measure. Dollars are a derived reading — **API-equivalent, at the prices of
the embedded snapshot** of [API Price Index](https://apipriceindex.com) (CC BY 4.0), dated
on every screen. Tokens sent through a subscription are shown as *included in plan*, never
as $0, and never added to the total.

## What it finds

| Severity | Kinds |
|---|---|
| **1 · act now** | `tracked-secret` — a key in clear in a file git tracks · `exposed-secret` — a key in clear other users of the machine can read |
| **2 · review** | `clear-secret` — a key in clear (`secrets.json`, `.bashrc`, an ignored `.env`) · `shared-key` — the same key in several places · `expired` — a token past its expiry · `double-credit` — a host on a subscription with an API key of the same provider in its environment · `unpriced` — a model the price table does not know |
| **3 · good to know** | `pruning-soon` — a usage log the host deletes within 7 days · `host-disagrees` — the host's own cost differs from Billfold's by more than 10 % · `near-limit` — Codex's last written quota at 80 % or more · `legacy-store` — a credential file the host no longer uses |
| **4 · information** | `no-usage-log` — a host present that writes no token log · `in-keyring` — secrets in the system keyring, not read |

Two things Billfold refuses to say: whether you spend *too much* (a figure, not a verdict),
and what is *left* on a plan or an account — that lives at the provider, except the quota
Codex writes to its own rollouts, shown as such.

## Hosts and providers

Twenty-one hosts, each from a sourced sheet in [`hosts/`](hosts): Claude Code, Codex CLI,
Gemini CLI, Qwen Code, OpenCode, Goose, Aider, Cline, Roo Code, Kilo Code, GitHub Copilot
(CLI and `gh`), Zed, Amp, Cursor, Kiro, Claude Desktop, Devin, JetBrains AI, Trae, Warp.
Eleven of them keep a per-request token log Billfold reads (JSONL, JSON, SQLite, Markdown),
each with its own deduplication rule — Claude Code writes one line per content block of a
response; Gemini writes each message twice; Codex replays a parent's prefix in a subagent;
Aider rounds its counts. The others are shown as present with the reason nothing is read.

Twenty-one providers, each from a sourced sheet in [`providers/`](providers): the variable
names their documentation gives (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
`GEMINI_API_KEY`…), the key shapes scanners have observed (attribution goes by name first,
by shape second), the page where usage is shown, and the price rules the index does not
carry — cache writes at 5 min / 1 h, long-context tiers, DeepSeek's peak hours, announced
changes.

## Use

```sh
billfold                        # the findings, most serious first
billfold wiring                 # the accounts each host is logged into, and how they pay
billfold keys                   # the provider keys on this machine, masked, with their git state
billfold meter                  # tokens and API-equivalent dollars, by model (--by host|project|day)
billfold meter --since 90       # a wider window (default 30 days — the retention of Claude Code)
billfold hosts                  # the hosts, their presence, where their rules come from
billfold providers              # the providers, their variables, their price rules
billfold --json                 # everything, for scripts
billfold report                 # bbl-report/1 for the Blackberrylabs control tower
billfold-app                    # the desktop app (--select <id>, --since <days>, --tab keys|meter)
```

Projects — for `.env` files and per-project configs — are the ones you name with `-p` plus
the projects Claude Code knows on this machine (`~/.claude.json`).

## Promises

Read-only (nothing written outside `~/.config/billfold/`, the app's preferences); no network
(the price table is a snapshot embedded at build time; provider pages are links, never
fetched); secrets masked the moment a line is read and never written — a key is shown as
`sk-ant-api03-…igAA` and compared across places through an HMAC with a salt that lives in
memory only; the system keyring is never opened; `git` is never run (the index and the
`.gitignore` files are read directly); actions shown, never run; every host and provider
rule cites its source, its date and the words it was read from (`docs/hosts.md`,
`docs/providers.md`). `#![forbid(unsafe_code)]`; `tests/promises.rs` checks the source for
writes and network calls, plants a key and checks it never comes out, and checks the report
for conformance.

## Part of Blackberrylabs

Billfold is a module of the [Blackberrylabs](https://github.com/blackberrylabs) family —
read-only tools that show what AI coding agents do to your machine. Want only Billfold?
It is complete on its own. Want several? The
[control tower](https://github.com/blackberrylabs/blackberrylabs) gathers what every
module finds in one list, most serious first: `billfold report` is what it reads, `billfold-app --select`
is how it opens Billfold at a finding.

## Build

```sh
cargo test
cargo build --release          # target/release/billfold, billfold-app
./packaging/build.sh           # .deb, .rpm, AppImage in dist/
```

Depends on [`bbl-hostrules`](https://github.com/blackberrylabs/bbl-hostrules) (locations,
readers, secret masking), [`bbl-report`](https://github.com/blackberrylabs/bbl-report) (the
tower's contract) and [`bbl-ui`](https://github.com/blackberrylabs/bbl-ui) (the family's face).
Prices: a snapshot of `https://apipriceindex.com/api/all.json`, CC BY 4.0, attribution
"API Price Index (apipriceindex.com)" — refreshed at each release, its `as_of` shown.

## Status

0.1.0 — first release. Packages (`.deb`, `.rpm`, AppImage, x86_64) on the
[releases page](https://github.com/blackberrylabs/billfold/releases). License MIT — Alexandre Do.
