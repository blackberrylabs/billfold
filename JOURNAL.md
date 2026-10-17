# JOURNAL — Billfold
<!-- Journal du projet (français), le plus récent en premier. Une entrée = une date, un titre, ce qui a été décidé, par qui, ce qui a été écarté, la preuve. -->

---

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
