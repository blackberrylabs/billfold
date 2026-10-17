# JOURNAL — Billfold
<!-- Journal du projet (français), le plus récent en premier. Une entrée = une date, un titre, ce qui a été décidé, par qui, ce qui a été écarté, la preuve. -->

---

## 2026-10-17 — release 0.1.0

- **Push bloqué par GitHub** (push protection) : la clé DeepSeek *factice* du faux HOME de
  test (`sk-` + 32 hexadécimaux, la forme exacte du fournisseur) a été prise pour une vraie.
  Pas de dérogation : la chaîne est désormais assemblée à l'exécution dans le test, et les
  cinq commits réécrits avant la première publication (aucun clone n'existait) ; la branche
  `master` renommée `main` comme les modules frères. Leçon pour la famille : une clé de
  test ne doit jamais avoir la forme exacte d'une vraie dans un fichier source.
- **Tag** `v0.1.0` sur le commit des notes de release (six commits). `packaging/build.sh`
  lancé par Alexandre : `.deb` 7,6 Mo, `.rpm` 8,2 Mo, AppImage 10,5 Mo, `SHA256SUMS` ;
  `appimagetool` repris de Permitt, aucun téléchargement.
- **Vérification depuis l'extérieur** : le `.deb` ouvert sans être installé (`ar x`) —
  deux binaires, `.desktop`, sept icônes, README et copyright, dépendances déclarées ; le
  binaire CLI identique dans le `.deb` et l'AppImage ; `billfold --version` = 0.1.0 ; le
  paquet lancé sur un Ubuntu 22.04 vierge : la CLI tourne (glibc 2.34), l'app réclame
  glibc 2.39 — limite notée au CHANGELOG, même situation que les modules frères construits
  sur 24.04.
- **Reste** : comparaison du paquet avec `target/debug` sur la machine d'Alexandre, puis
  push et release GitHub (son accord).

## 2026-10-17 — premier passage réel : ce que la machine d'Alexandre a appris à Billfold

- **Juste du premier coup** : les cinq hôtes présents (Claude Code, Claude Desktop, Cursor,
  GitHub Copilot, OpenCode), deux clés dans un `.env` de projet (600, ignoré par git,
  masquées), Cursor expiré, Claude Code à 99 % de cache (96 M lus, 218 requêtes ≈ 30 $
  d'équivalent API, couverts par le plan).
- **Faux, corrigés (commit 2)** : `$-0.00` (somme vide de flottants) ; `billingType` pris
  pour le nom du plan — c'est un mode (abonnement ou clé) ; un nom d'organisation généré
  depuis une adresse (`…@domaine` seul gardé) ; OpenCode « unknown » sur 1 400 requêtes :
  la version courante écrit `data.model.id` / `data.model.providerID` (son `stats.ts` fait
  foi), le lecteur suit les deux formes et saute les forks par date de création de session ;
  « cursor » en minuscules → nom d'affichage.
- **Faux, corrigés (commit 3)** : cinq « non tarifés » qui étaient des modèles **gratuits**
  d'OpenCode Zen (`space-bunny-free`, `muse-spark-1.3-contributor-free`, `longcat-2.5-preview-
  free`, `glm-5.3-flash-free`, `deepseek-v4.1-flash-free`). Nouvelle fiche
  `providers/opencode.toml` : règle `pricing.free` (suffixe `-free`, `big-pickle`), source
  opencode.ai/docs/zen (« Use `jev-1.13-free` instead of `jev-1.13` to use the limited-time
  free model. ») ; ligne tarifée 0 avec la note `free model`, jamais « included in plan » ;
  modèles payants au prix du fournisseur d'origine (« selling at cost »). Nouveau champ
  `login_credit = "either"` : une connexion OAuth à Zen paie en crédits ou par OpenCode Go,
  le disque ne dit pas lequel → crédit inconnu, avec la note. Le constat « non tarifé »
  compte désormais les mêmes requêtes que la table (une requête à zéro jeton n'en changeait
  le compte que d'un côté) et écrit ses jetons en `k`/`M`.
- **Plan Claude Code (commit 4)** : sur cette machine `~/.claude/.credentials.json` ne
  contient que `mcpOAuth`, pas `claudeAiOauth` ; le plan est dans `.claude.json`,
  `oauthAccount.organizationType` (`claude_max`…), source publique anthropics/claude-code
  #58625 ; un nom de plan n'écrase jamais un `billingType` qui dit « api ».
- **Le passage 3 a trouvé une vraie clé** : `OPENCODE_ZEN_KEY` en ligne 1 de `~/.zshrc`,
  mode 664, home traversable par les autres → EXPOSED (act now), attribuée par son nom
  (« OpenCode Zen ») ; l'alias documenté `OPENCODE_ZEN_API_KEY` ajouté à la fiche, le
  préfixe `oc_sk_` n'étant documenté nulle part de public.

## 2026-10-17 — cœur, CLI, app : tout tient sur le faux HOME

- **Fait** : `bbl-hostrules` 0.5.0 (trois formats, hôte par référence) ; 21 fiches hôtes et
  21 fiches fournisseurs ; `wiring.rs`, `keys.rs`, `gitstat.rs` (index et `.gitignore` lus
  sans lancer git — le test de promesse interdit `Command::new`), `meter/` (12 lecteurs),
  `prices.rs`, `scan.rs`, `report.rs` ; CLI complète ; app à trois onglets.
- **Preuve** : 28 tests ; `tests/promises.rs` plante une clé Anthropic dans `.bashrc` et un
  jeton dans `.credentials.json`, vérifie qu'aucun n'apparaît dans l'inventaire ni le rapport
  et que l'arbre de fichiers est intact ; `tests/scan.rs` construit un faux HOME (Claude Code
  avec sous-agent, Codex `.jsonl` avec quota à 91 %, OpenCode SQLite avec fork et compte
  expiré, `.env` suivi par git, Goose en trousseau, Cursor sans journal) et exige chaque
  constat une fois, les jetons justes, DeepSeek chiffré en heures creuses ou pleines.
- **Décisions de détail** : l'exposition d'une clé se juge sur les droits *autres* du
  fichier **et** de tous ses dossiers parents (un `.bashrc` 644 dans un home 700 n'est pas
  exposé) ; le masque garde le préfixe jusqu'à son dernier séparateur dans les 14 premiers
  caractères (`sk-ant-api03-…igAA`, `sk-proj-…bbbb`, `gsk_…MNOP`) ; la tour n'acceptant que
  quatre chiffres, le nombre de fournisseurs passe en note.
- **Maquette A1** validée par Alexandre (« Ça me va, tu peux continuer ») ; dans l'app, les
  tableaux tiennent sur deux lignes par rangée pour la colonne de gauche.
- **Écarté pour la 0.1** : le rejeu du préfixe des sous-agents Codex (comptés deux fois dans
  le rare cas d'un sous-agent), le `cost-state` de Claude Code (cumul par session, sans
  date), le trousseau.

## 2026-10-17 — l'index corrigé avant la 0.1

- Alexandre choisit **l'index corrigé avant la 0.1** plutôt qu'une 0.1 à trous. La
  conversation Apipriceindex livre dans la journée : `pricing.tier` (standard, off_peak,
  peak, le_100k…), `api.model` + `api.aliases` (ids Bedrock, Vertex, datés), prix de base et
  remise, six corrections, neuf modèles ajoutés (Gemini 2.5/3.x, GPT-4.1, gpt-5.3-codex,
  Claude 4.x). `as_of` = heure du build, pas des prix (relevé quotidien 06:00 UTC).
- Billfold tient en fiches ce que l'index ne modélise pas : écriture en cache, paliers,
  heures pleines, lots, hausses annoncées.

## 2026-10-17 — naissance

- **L'idée d'Alexandre** : un module qui liste « tous les abonnements qu'un développeur ou un
  vibe codeur aurait — Claude, OpenAI, DeepSeek, tous les providers — et même peut-être le
  montant restant ».
- **Reformulé et validé** : pas de solde ni d'abonnement distant (réseau et secrets
  interdits par la famille, et la plupart des fournisseurs n'exposent pas de solde) ; à la
  place, ce que la machine sait en lecture seule : les comptes branchés, les clés, la
  consommation locale en **jetons** (« bien sûr »), les dollars en lecture dérivée.
- **Décisions** : B1 nom **Billfold** (portefeuille : cartes et reçus), verbe `tally`,
  recommandation retenue ; B2 treize constats sur l'échelle de Linstruct ; B3 l'index
  Apipriceindex seul + fiches ; B4 fenêtre 30 jours, hôte → modèle → projet → jour, quatre
  colonnes de jetons + part de cache ; B5 pas de D-Bus en 0.1.
- **Honnêteté notée dans le BRIEF** : ccusage compte déjà les jetons de neuf hôtes ; le
  produit de Billfold, ce sont le branchement, les clés et les constats, pas le compteur.
- **Sources** : quatre rapports de sous-agents (A connexions, B journaux, C clés, D prix),
  note sur `all.json`, synthèse ; conservés dans `00_cadrage/`.
