# PLAN — Billfold
<!-- Plan du projet (français). -->

## Étape 0. Sources — fait le 2026-10-17
- [x] Quatre dossiers (connexions des hôtes, journaux de consommation, clés sur une machine,
      prix et abonnements), note sur le jeu de données Apipriceindex, synthèse
- [x] Deux allers-retours avec la conversation Apipriceindex : vérification de l'index,
      puis ajouts livrés (`tier`, `api`, base/remise, corrections, modèles manquants)

## Étape 1. Cadrage — fait le 2026-10-17
- [x] Nom vérifié (crates.io, PyPI, npm libres) et validé : **Billfold**, verbe `tally`
- [x] BRIEF rev. 1 validé ; décisions B0–B5
- [x] Dépôt `blackberrylabs/billfold` créé par Alexandre ; dossier `Logiciels/Billfold`

## Étape 2. Cœur — fait le 2026-10-17
- [x] `bbl-hostrules` 0.5.0 (JSONL, dotenv, SQLite ; hôte par référence)
- [x] 21 fiches hôtes, 21 fiches fournisseurs, sourcées
- [x] Branchement (comptes, plan, domaine, expiration, JWT local), clés (10 emplacements,
      masque, empreinte de session, état git sans lancer git)
- [x] 12 lecteurs de journaux avec leurs règles de dédoublonnage
- [x] Prix : instantané de l'index + règles des fiches (cache 5 min / 1 h, paliers, heures
      pleines, hausses annoncées), jointure par id API / alias / base_model
- [x] 13 constats ; tests : promesses (clé plantée), faux HOME (un constat de chaque sorte),
      unitaires par lecteur — 28 tests, clippy propre

## Étape 3. CLI et rapport — fait le 2026-10-17
- [x] `billfold`, `wiring`, `keys`, `meter --by`, `hosts`, `providers`, `report` ; `--json`,
      `--home`, `--host`, `--since`, `-p`
- [x] `bbl-report/1` conforme (quatre chiffres : clés, jetons, dollars, constats)

## Étape 4. App — fait le 2026-10-17
- [x] Maquette A1 validée (Wiring, Meter)
- [x] App iced sur bbl-ui : trois onglets, constats, détail, `--select`, `--tab`, `--since`,
      EN/FR

## Étape 5. Passage réel — à faire
- [ ] Relevé d'existence sur la machine d'Alexandre (commande donnée), puis `billfold`,
      `billfold wiring`, `billfold keys`, `billfold meter`
- [ ] Corrections d'après le réel

## Étape 6. Release 0.1.0 — sur accord
- [ ] Instantané de l'index du jour (as_of du build 14:15), `bbl-hostrules` v0.5.0 poussé et
      tagué d'abord
- [ ] Paquets (.deb, .rpm, AppImage), tag, release vérifiée depuis l'extérieur
- [ ] `modules/billfold.toml` dans la tour (0.3.0), docs de la famille, profil de
      l'organisation, `00_moi`

## Plus tard (0.2)
- Trousseau par D-Bus (`SearchItems` + étiquettes, jamais `GetSecret`) après essai réel
- Historique des prix (chiffrer au tarif de l'époque) quand `pricing-history.csv` le permet
- Hôtes : Antigravity, Devin desktop, Copilot VS Code si un journal apparaît
- Préfixe des sous-agents Codex rejoué (marqueurs `task_started`), compaction Codex
