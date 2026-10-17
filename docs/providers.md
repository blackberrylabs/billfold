# Provider sheets — `providers/*.toml`

One file per AI provider: how its keys are named and shaped, where usage is shown, and the
price rules the embedded API Price Index snapshot does not carry.

| Field | Meaning |
|---|---|
| `id`, `name` | Billfold's provider id and display name. |
| `env_vars` | The variable names the provider's documentation gives (`ANTHROPIC_API_KEY`). **Primary attribution**: a key found under one of these names belongs to this provider. |
| `config_names` | Names seen in tool configuration files (`anthropic-api-key`, `anthropicApiKey`), compared without case, dashes, underscores or dots. |
| `shapes[].regex`, `shapes[].source` | Anchored regexes of a whole key, as scanners have observed them — **secondary attribution**, for a key found under an unknown name. A provider with no documented or observed shape has none: its keys are attributed by name only. |
| `usage_url` | The provider's usage or billing page (`open-url`, never fetched). |
| `login_credit` | What an OAuth login pays with when the host's store does not say: `subscription` (default) or `either` — credits or a plan, kept as an unknown credit with a note. |
| `id_prefixes` | Model-id prefixes to strip before matching the price table (`anthropic.`, `us.anthropic.`, `models/`). |
| `aliases` | `[[from, to]]` pairs: an id a host logs → the id the price table knows (`deepseek-flash` → `deepseek-v4.1-flash`). |
| `index_ids` | The provider ids of the price table this provider's models are priced under. |
| `pricing.cache_write_5m`, `.cache_write_1h` | Cache write price as a multiple of the input price, by lifetime. Without them, cache writes are priced as input and the line says so. |
| `pricing.reasoning_as_output` | Reasoning tokens are billed as output (already inside `output` in the logs). |
| `pricing.long_context[]` | `{models, above, input, output, cache_read}`: prices above a prompt size, per million tokens, when the index stores the lower tier (`tier = le_…`). |
| `pricing.off_peak` | `{windows[{days,start,end}], peak_multiplier}`: the index stores the off-peak rate (`tier = off_peak`); inside the windows (UTC, the **peak** hours) the price is multiplied. |
| `pricing.announced[]` | `{models, from, input, output, cache_read, quote}`: a price change already announced, applied to requests dated on or after `from`. |
| `pricing.free` | `{suffixes, models, source}`: model ids the provider serves for nothing (the index lists no $0 row); matched by suffix (`-free`) or exact id after the prefixes are stripped; priced 0 with the note `free model`, never "included in plan". |
| `pricing.source`, `pricing.note` | Where the rules were read; what is noted but not applied (batch, flex, fast modes, regional prices). |
| `source` | The sheet's primary source (the page that names the variables). |

Prices themselves are not in the sheets: they come from `prices/apipriceindex.json`, a
snapshot of `https://apipriceindex.com/api/all.json` (CC BY 4.0, attribution "API Price
Index (apipriceindex.com)") taken at each release, with `schema_version` 1, `as_of`, and
per model `api.model`, `api.aliases`, `base_model`, `pricing.{input_per_mtok,
output_per_mtok, cached_input_per_mtok, tier, discount, verified_at, source_url}`. The
join from a logged model id goes: as written → provider prefixes stripped → `author/`
prefix stripped → `:batch`-style suffix stripped → date suffix stripped → sheet aliases;
each key is looked up in `api.model` and `api.aliases`, then in `base_model` and the index
slug. A model that matches nothing is `unpriced`: tokens counted, dollars not invented.
