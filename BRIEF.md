# BRIEF — Billfold (rev. 1, validé le 2026-10-17)

Module n° 6 de la famille Blackberrylabs. Verbe : **tally**. Une phrase :

> **Billfold montre ce que ce poste dépense en IA : les comptes auxquels il est branché, les
> clés qu'il garde, les jetons qu'il a brûlés — et ce que ça vaudrait au tarif API.**

Tagline (anglais, carte de la tour) : *What this machine spends on AI — the accounts it is
wired to, the keys it holds, the tokens it burned.*

Un *billfold* est un portefeuille : les cartes (comptes, clés) et les reçus (consommation)
dans un seul objet. Nom libre sur crates.io, PyPI et npm (vérifié le 2026-10-17) ; une
société « Billfold » existe dans le paiement sans contact pour festivals (billfold.tech),
domaine sans rapport.

## 1. Le problème

Un développeur ou un vibe codeur accumule des fournisseurs (Anthropic, OpenAI, Google,
DeepSeek, Mistral, OpenRouter…), des comptes (un abonnement Claude Max, un plan ChatGPT, une
clé API par-ci par-là) et des outils qui les consomment (Claude Code, Codex, Cursor, Gemini
CLI, OpenCode, Aider, Cline…). Trois questions n'ont pas de réponse simple sur son poste :

- **À quoi suis-je branché ?** Quel outil utilise quel compte, avec quel type de crédit
  (abonnement ou clé), et depuis quand. Chaque hôte le range à sa manière : JSON en clair
  (`~/.claude/.credentials.json`, `~/.codex/auth.json` — « treat it like a password »), SQLite
  (OpenCode, Zed, Kiro, Cursor), trousseau système (Gemini CLI, Goose, Copilot CLI, `gh`), et
  certains ont changé de méthode sans que la doc suive (sources A).
- **Où sont mes clés, et qui les voit ?** Dans l'environnement, `.bashrc`, un `.env` de
  projet — parfois **suivi par git** —, `secrets.json` de Cline en clair, `settings.json` de
  Qwen, une config MCP, `.aider.conf.yml`. La même clé copiée dans trois outils ; une clé
  d'un fournisseur abandonné qui traîne (sources C).
- **Combien ai-je consommé ?** Les hôtes journalisent les jetons localement — onze d'entre
  eux, chacun dans un format différent avec ses pièges (Claude Code écrit la même réponse une
  fois par bloc ; Gemini deux fois ; Codex rejoue le préfixe des sous-agents ; Aider arrondit)
  — mais aucun ne les montre ensemble, et **Claude Code purge ses journaux à 30 jours**
  (sources B). Les dollars n'existent nulle part pour un abonnement, et le solde restant
  n'existe que chez le fournisseur.

Ce qui existe : **ccusage** (réécrit en Rust) compte les jetons de neuf de ces hôtes avec des
prix embarqués — la moitié « compteur » est donc prouvée faisable, et Billfold ne prétend pas
l'inventer. Personne ne fait l'inventaire des comptes et des clés, les constats (clé en clair,
`.env` suivi, clé partagée, abonnement et clé en double, purge imminente), la lecture
« abonnement = inclus » plutôt que « 0 $ », ni le branchement à une tour. Le compteur est le
décor ; les constats sont le produit.

## 2. Ce que fait Billfold

Pour chaque **hôte présent sur la machine**, Billfold lit en lecture seule :

1. **Le branchement** (*wiring*) : le ou les comptes configurés — fournisseur, type de crédit
   (`subscription` avec le plan quand il est lisible hors ligne : Claude Code
   `subscriptionType`, Codex `chatgpt_plan_type` décodé du JWT local ; `api-key` ;
   `keyring` quand la fiche dit que c'est là, sans lecture), domaine de l'e-mail, date
   d'expiration, fichier et droits. Jamais le jeton, jamais l'e-mail complet.
2. **Les clés** (*keys*) : chaque clé de fournisseur trouvée dans les emplacements connus
   (environnement du processus, `~/.bashrc` `~/.zshrc` `~/.profile`,
   `~/.config/environment.d/*.conf`, `/etc/environment`, `.env*` et `.envrc` des projets
   connus, configs d'outils, configs MCP, `~/.netrc`, `.npmrc`, `~/.aws/credentials`,
   `~/.cache/huggingface/token`), attribuée **d'abord par le nom** de la variable ou de la
   clé de config, ensuite par la forme (préfixe, marqueur `T3BlbkFJ`), avec : fournisseur,
   forme masquée (`sk-ant-api03-…igAA`, jamais plus de 4 caractères finaux), emplacement,
   droits, suivie ou ignorée par git, et une **empreinte de session** (HMAC salé en
   mémoire, jamais écrit) qui permet de dire « même clé dans Aider et OpenCode ». Une
   référence (`${VAR}`, `{env:…}`, `os.environ/…`, `op://…`, `ENC[…]`) n'est jamais une clé.
3. **La consommation** (*meter*) : les jetons de chaque journal local, dédoublonnés selon la
   règle de l'hôte, **en jetons d'abord** — entrée, sortie, écriture en cache, lecture en
   cache, raisonnement quand il est séparé — regroupés **hôte → modèle → projet → jour**,
   sur les **30 derniers jours** par défaut (`--since` pour élargir), avec la **part de cache**
   en pourcentage. Puis, dérivé et affiché comme tel, le **dollar équivalent API** au tarif de
   l'instantané embarqué (`as_of` affiché à côté de chaque total) ; pour un hôte branché sur
   un abonnement, la colonne dit **« included in plan »**, jamais 0 $. Quand l'hôte a calculé
   son propre coût (OpenCode, Goose, Aider, Cline/Roo/Kilo), il est montré en regard. Pour
   Codex seulement, le **dernier quota connu** tel que l'hôte l'a écrit (`used_percent`,
   `resets_at`, fenêtre), daté.

De là, les constats.

### Constats (`Kind`) et gravité

| Kind | Ce que c'est | Gravité |
|---|---|---|
| `tracked-secret` | Une clé en clair dans un fichier **suivi par git** (`git ls-files --error-unmatch` = 0) : `.env`, `.aider.conf.yml`, `opencode.json`, `.mcp.json`… | **1 act now** |
| `exposed-secret` | Une clé en clair dans un fichier lisible par d'autres utilisateurs (droits plus larges que 0600, ou dossier parent ouvert) | **1 act now** |
| `clear-secret` | Une clé en clair, droits corrects : Cline `secrets.json`, Qwen `settings.json` `env`, `.bashrc`, `.env` ignoré par git, `~/.aider/oauth-keys.env`, bloc `env` d'un serveur MCP | **2 review** |
| `shared-key` | La même clé (empreinte de session) dans plusieurs outils ou fichiers | **2 review** |
| `expired` | Jeton ou session dont `expiresAt` / `exp` est passé (Claude Code, Codex, Gemini legacy, Kilo, OpenCode) | **2 review** |
| `double-credit` | Un hôte connecté par abonnement **et** une clé API du même fournisseur dans son environnement — Claude Code : `/status` « marks the credential that isn't in use » ; Codex : `auth_mode` vs `OPENAI_API_KEY` | **2 review** |
| `unpriced` | Un modèle consommé absent de la grille embarquée : jetons comptés, dollars impossibles (avec l'identifiant tel que le journal l'écrit) | **2 review** |
| `pruning-soon` | Journal de jetons qui sera purgé par l'hôte dans moins de 7 jours (Claude Code `cleanupPeriodDays`, Gemini `sessionRetention.maxAge`, Qwen `cleanupPeriodDays`) — avec ce qui part | **3 good to know** |
| `host-disagrees` | Le coût écrit par l'hôte s'écarte du nôtre de plus de 10 % (grille d'époque, promo, palier) | **3 good to know** |
| `near-limit` | Dernier quota connu de Codex ≥ 80 % sur une fenêtre, avec `resets_at` | **3 good to know** |
| `legacy-store` | Fichier de connexion que l'hôte n'utilise plus : `~/.local/share/opencode/auth.json`, `~/.gemini/oauth_creds.json`, `~/.kilocode/cli/config.json` | **3 good to know** |
| `no-usage-log` | Hôte présent qui n'écrit aucun compteur local (Cursor, Copilot VS Code, Warp, Trae, JetBrains, Claude Desktop chat) — où le voir chez le fournisseur | **4 information** |
| `in-keyring` | Secrets rangés dans le trousseau système d'après la fiche : rien à lire, rien à craindre | **4 information** |

Deux refus explicites : **aucun jugement sur le montant** (« trop cher » n'est pas un
constat) ; **aucun solde ni restant** autre que celui que Codex écrit lui-même — pour les
autres, « not available offline » et un lien `open-url` vers la page d'usage du fournisseur.

Hors périmètre v0.1 : le trousseau par D-Bus (0.2, après essai sur une machine réelle) ;
l'historique des prix (chiffrage au tarif de l'instantané, pas de l'époque) ; les modèles
locaux (Ollama, LM Studio : pas de journal, pas de prix) ; macOS et Windows (chemins notés
dans les fiches, non testés).

### Chiffres du résumé (cartes de la tour)

`keys` (clés trouvées), `tokens_30d` (jetons des 30 derniers jours, hôtes présents),
`usd_30d` (équivalent API, hors abonnements — avec `as_of`), `findings`. Le contrat de la
tour n'accepte que quatre chiffres : le nombre de fournisseurs passe en note.

### Actions (affichées, jamais exécutées)

`open-file` sur le fichier et la ligne (clé, journal) ; `hint` (« add `.env` to
`.gitignore` and rotate the key », « `chmod 600 …` », « remove `OPENAI_API_KEY` from
`~/.bashrc`, Codex is on the Plus plan », « set `cleanupPeriodDays`… ») ; `open-url` vers la
page d'usage ou de facturation du fournisseur, et vers la source citée par la fiche ;
`command` pour les vérifications de l'hôte (`claude` puis `/status`, `/usage` ; `codex login
status`, `/status` ; `gemini` puis `/stats` ; `opencode auth list` ; `gh auth status`).

## 3. Hôtes et fournisseurs (v0.1)

Deux jeux de fiches sourcées (doc, code source ou `observed` : URL, date, citation) :

**`hosts/<id>.toml`** — où l'hôte range ses connexions, ce qui s'y lit (champs sûrs) et ce
qui est secret ; où il journalise, dans quel format, avec quelle clé de dédoublonnage et
quelle rétention ; s'il écrit un coût ; s'il écrit un quota. Présence : dossier de config
ou binaire, comme Linstruct.

| Hôte | Connexion lue | Journal lu | Dédoublonnage | Coût hôte | Quota |
|---|---|---|---|---|---|
| Claude Code | `.credentials.json` (plan, tier, expiry), `~/.claude.json` `oauthAccount` (domaine, org, `billingType`), env/`apiKeyHelper`/profils | `~/.claude/projects/**/*.jsonl` + `subagents/` ; `cache_creation.{5m,1h}`, `thinking`, `speed`, `inference_geo` | (`message.id`, `requestId`) ; sidechain écartée ; `<synthetic>` sauté | `cost-state` (comparaison) | non |
| Codex CLI | `auth.json` ou trousseau ; `auth_mode`, JWT → plan, domaine, `exp` | `~/.codex/sessions/**/rollout-*.jsonl[.zst]`, `archived_sessions/` ; `token_count` deltas ; modèle via `turn_context` | cumul → delta ; `token_usage_record` par `response_id` ; préfixe sous-agent | non | **oui** `rate_limits` |
| Gemini CLI | trousseau (fiche), `gemini-credentials.json`, legacy `oauth_creds.json` ; `google_accounts.json` domaine ; `selectedType` | `~/.gemini/tmp/*/chats/session-*.jsonl` `tokens.*` | dernier par `id` | non | non |
| Qwen Code | `settings.json` (`selectedType`, `modelProviders`, `env` = clés) ; legacy `oauth_creds.json` | `~/.qwen/projects/*/chats/*.jsonl` `usageMetadata`, `authType` | par ligne | non | non |
| OpenCode | SQLite `credential` (type, expiry) ; legacy `auth.json` ; `opencode.json` modèle | même base `session_message` ; `cost` | lignes de fork ≤ `seq` | oui | non |
| Goose | `config.yaml` (`active_provider`, modèles) ; trousseau (fiche) / `secrets.yaml` | SQLite `usage_ledger` ; `cost_source` | `is_compaction` | oui | non |
| Aider | `.aider.conf.yml` ×3, `.env` ×3, `oauth-keys.env` | `.aider.chat.history.md` par projet (arrondi : dit tel quel) | — | oui (texte) | non |
| Cline | `globalState.json` (providers, `configuredApiKeys`, domaine) ; `secrets.json` (clair) | `tasks/*/ui_messages.json` `api_req_started` ; `~/.cline/data/sessions/*/*.json` | `ts` | oui | non |
| Roo Code | `listApiConfigMeta` (clair) ; profils chiffrés (`in-keyring`) | `tasks/*/ui_messages.json` + `apiProtocol` | `ts` | oui | non |
| Kilo Code | `~/.local/share/kilo/auth.json` | SQLite `kilo*.db` `message` | id | oui | non |
| Copilot CLI / `gh` | trousseau (fiche) / `~/.copilot/config.json` ; `hosts.yml` (`user`) | `~/.copilot/session-state/*/events.jsonl` `session.shutdown` (cumul par modèle, `inputTokens` inclut le cache) | soustraire le shutdown précédent | crédits (AIU) | non |
| Zed | `settings.json` `default_model` ; trousseau (fiche) | SQLite `threads.db` blob JSON/zstd `request_token_usage` ; `zed.dev` = plan | thread + message | non | non |
| Amp | `settings.json` ; secrets (fiche, `observed`) | `~/.local/share/amp/threads/T-*.json` `messages[].usage` | message id | crédits | non |
| Cursor | `state.vscdb` `cursorAuth/cachedEmail`, `exp` (`observed`) | aucun → `no-usage-log` | — | — | non |
| Kiro | SQLite `data.sqlite3` (prédécesseur, `observed`) | crédits seulement → `no-usage-log` | — | — | non |
| Copilot VS Code, Warp, Trae, JetBrains, Claude Desktop (chat), Devin | présence + fiche « non documenté » | aucun → `no-usage-log` | — | — | non |
| Ollama / LM Studio | présence (modèles locaux, pas de compte) | aucun | — | — | — |

**`providers/<id>.toml`** — pour chaque fournisseur : noms de variables documentés
(`ANTHROPIC_API_KEY`…), formes observées (regex, scanner cité), page d'usage/facturation
(`open-url`), et les **règles de prix que l'index ne porte pas** : écriture en cache 5 min /
1 h (Anthropic 1,25× / 2× ; Kimi 1× / 2× ; DashScope 125 % ; OpenAI une durée), paliers long
contexte (quand `tier` = `le_…`, le tarif au-delà), heures pleines DeepSeek (`off_peak` ×2
sur 01:00–04:00 et 06:00–10:00 UTC en semaine), raisonnement facturé en sortie, lots et modes
rapides (notés, non appliqués en 0.1 faute de marqueur dans les journaux sauf Claude Code
`speed`/`service_tier`), hausses annoncées (Gemini 3.6/3.8 Flash au 01/01/2027).
Fournisseurs v0.1 : Anthropic, OpenAI, Google (Gemini, Vertex), DeepSeek, Mistral, xAI,
Groq, OpenRouter, Together, Fireworks, Perplexity, Cohere, Hugging Face, Cerebras,
Moonshot, Z.ai, DashScope, MiniMax, GitHub, AWS Bedrock, Azure OpenAI.

**`prices/apipriceindex.json`** — instantané de `https://apipriceindex.com/api/all.json`
(CC BY 4.0, attribution « API Price Index (apipriceindex.com) » affichée, `as_of` affiché),
pris **après** les ajouts livrés côté index le 2026-10-17 : `pricing.tier`, `api.model` +
`api.aliases`, prix de base et remise, corrections, modèles manquants (808 modèles dans
l'instantané du jour). Jointure journal →
prix : identifiant du journal normalisé (préfixes Bedrock/Vertex, suffixes de date, `:batch`)
→ `api.model` ou `api.aliases` → sinon `base_model` → sinon `unpriced`. Le fichier porte sa
date ; la tour et l'app la montrent.

## 4. Promesses (celles de la famille)

R1 lecture seule, sans exception ; R2 aucune sortie réseau (les pages d'usage sont des
liens, jamais suivis ; l'index est un instantané) ; R3 secrets masqués à la lecture, jamais
écrits, jamais dans le JSON ni le rapport (forme masquée et empreinte de session seulement ;
la clé en clair ne quitte jamais `bbl_hostrules::secrets`) ; R4 rien d'écrit hors
`~/.config/billfold/` (préférences de l'app) ; R5 actions affichées, jamais exécutées ;
R6 chaque règle d'hôte et de fournisseur citée (source, date, citation) ; R7 un état, pas
une note — pas de score, pas de « trop ».

Précision R3 : un JWT est décodé localement pour ses **revendications** (plan, domaine, `exp`)
et jamais affiché ; `secret-tool` n'est jamais appelé (il imprime les secrets).

## 5. Architecture

Depuis `blackberrylabs/template` (`rename.sh billfold "Billfold"`). Mêmes dépendances que
Linstruct (bbl-hostrules 0.4.0, bbl-report, bbl-ui, iced 0.13) plus `rusqlite` (bundled,
lecture `?mode=ro`, copie en tempfile si la base est verrouillée), `zstd` (Codex `.jsonl.zst`,
Zed), `hmac`/`sha2` (empreinte de session), `jsonwebtoken` ou décodage base64 maison (sans
vérification de signature, hors ligne).

- `bbl-hostrules` **0.5.0** : `Format::Jsonl`, `Format::Dotenv`, `Format::Sqlite` (décrit,
  lu par le pilote du module) ; un hôte actif peut n'avoir qu'une référence quand il ne
  laisse aucun fichier lisible (Trae, JetBrains, Warp). À la source, pas une copie.
- `crates/core` (`billfold-core`) : `scan(env)` → `Inventory { hosts, accounts, keys,
  usage (lignes hôte × modèle × projet × jour), quotas, findings, not_read }` ;
  `hosts.rs` (fiches hôtes), `providers.rs` (fiches fournisseurs), `prices.rs` (instantané,
  jointure, règles), `wiring.rs`, `keys.rs`, `meter/<host>.rs` (un lecteur par format),
  `report.rs` → `bbl-report/1`. Ids stables : `<kind>/<host|provider>/<chemin relatif au
  home>[/<détail>]`.
- `crates/cli` : `billfold` (constats, les plus graves d'abord), `billfold wiring`,
  `billfold keys`, `billfold meter [--since 90d] [--by host|model|project|day]`,
  `billfold hosts`, `billfold providers`, `billfold report` ; `--json`, `--home`, `--host`,
  `-p DIR…` (projets pour `.env`/Aider, même règle que Linstruct : dossiers remémorés +
  projets Claude Code).
- `crates/app` : en-tête (une phrase : « 4 providers, 9 keys, 2.1 M tokens in 30 days ≈
  $38 API-equivalent, 3 to act on ») ; trois onglets **Wiring** (hôte → comptes, type de
  crédit, expiry), **Keys** (clé masquée → où, droits, git, outils qui la partagent),
  **Meter** (jetons par hôte/modèle/projet/jour, part de cache, dollars dérivés avec
  `as_of`) ; colonne de droite : le constat choisi (What it means / Where / What you could
  do / Check in <host> / Source). `--select <id>`. Même visage (bbl-ui), EN par défaut, FR en
  option.
- Tests : faux `HOME` avec un fichier de chaque format (dont un rollout Codex `.zst`, une base
  OpenCode, un `threads.db` Zed), un cas par Kind, un `.env` suivi dans un dépôt git de
  fixture, un jeu de jetons connus dont le dollar est vérifié à la main contre la page
  officielle ; `tests/promises.rs` (pas d'écriture hors config, pas de réseau, aucune clé en
  clair dans la sortie JSON — test qui injecte une fausse clé et vérifie son absence).

## 6. Étapes

0. Sources — fait (`sources_A…D`, note Apipriceindex, `synthese_sources.md`, réponses de
   l'index).
1. BRIEF validé ; décisions B1–B5 ; dossier `Logiciels/Billfold`, dépôt
   `blackberrylabs/billfold`.
2. Fiches hôtes (21) et fournisseurs (21) sourcées ; `wiring` + `keys` ; tests.
3. `meter` : un lecteur par format, dédoublonnage testé ; prix (instantané corrigé de
   l'index, `as_of` fourni par la conversation Apipriceindex) ; constats.
4. CLI + `report` conforme.
5. Maquette de l'app validée, puis l'app.
6. Passage réel sur la machine d'Alexandre (relevé d'existence d'abord, puis Billfold).
7. Release 0.1.0 (module d'abord), `modules/billfold.toml` dans la tour (0.3.0), docs des
   dépôts, profil de l'organisation.

## 7. Décisions prises (2026-10-17)

- **B0 — Périmètre** : comptes et clés branchés + consommation locale chiffrée ; **pas de
  solde ni d'abonnement distant** (réseau et secrets interdits) — le restant n'est montré que
  là où l'hôte l'écrit lui-même (Codex). Consommation **en jetons** ; les dollars sont une
  lecture dérivée.
- **B1 — Le nom : Billfold**, verbe `tally`. Dossier `Logiciels/Billfold`, dépôt
  `blackberrylabs/billfold`, binaires `billfold` et `billfold-app`.
- **B2 — Les treize constats** et leur gravité (tableau ci-dessus) ; pas de jugement sur le
  montant ; pas de solde.
- **B3 — Prix : l'index Apipriceindex seul**, instantané pris après ses ajouts
  (`tier`, `api`, base/remise, corrections, modèles manquants), + fiches fournisseurs pour
  l'écriture en cache, les paliers, les heures pleines, le raisonnement ; pas de LiteLLM.
  La 0.1 attend l'instantané corrigé.
- **B4 — Fenêtre 30 jours** par défaut (`--since`), regroupements hôte → modèle → projet →
  jour, quatre colonnes de jetons + part de cache en %.
- **B5 — Trousseau : 0.1 sans D-Bus** (`in-keyring` d'après la fiche) ; D-Bus en 0.2 après
  essai réel, jamais `GetSecret`, jamais `secret-tool`.
