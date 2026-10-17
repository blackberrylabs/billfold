# CLAUDE.md — Billfold
Billfold : ce que ce poste depense en IA — les comptes auxquels il est branche, les cles
qu'il garde, les jetons qu'il a brules — et ce que ca vaudrait au tarif API. Module n° 6 de
la famille Blackberrylabs ; fiche dans `Projets/00_moi/PROJETS.md`.

## Regles de travail

- BRIEF valide avant le code ; questions a Alexandre une par une, decisions une par une
  avec une recommandation ; PLAN et JOURNAL tenus a chaque etape ; `00_cadrage/` hors git.
- Lecture seule sans exception, aucune sortie reseau (la grille de prix est un instantane
  embarque), secrets masques et jamais ecrits, trousseau jamais ouvert, `git` jamais lance,
  rien d'ecrit hors `~/.config/billfold/`. Un etat, pas une note : jamais « trop cher »,
  jamais un solde invente. Actions affichees, jamais executees. Chaque regle cite sa source
  (doc, code source ou `observed`), sa date, sa citation : rien de memoire.
- Jetons d'abord ; dollars = equivalent API a la date de l'instantane, affichee ; une ligne
  sur abonnement est « included in plan », jamais 0 $, jamais dans le total.
- Commits locaux libres ; `git push`, suppression de fichiers, tags, timers : accord
  explicite. Commandes donnees completes, chemin absolu
  (`cd /home/nushoko/Bureau/Logiciels/Billfold && …`). Avant tout git lance par Alexandre :
  `find .git -name '*.lock' -delete` (le pont laisse des verrous).
- Anglais : code, commentaires, commits, README, interface par defaut. Francais : BRIEF,
  PLAN, JOURNAL, CLAUDE.md, option de langue.
- Seul nom cite : Alexandre Do.
- Un passage reel sur la machine d'Alexandre avant chaque release ; chaque release
  verifiee depuis l'exterieur (paquet telecharge, somme, version, `report` conforme).

## Identite git

Avant le **premier commit** : `git config user.name "Alexandre Do"` et
`git config user.email "277202463+Nimeria2026@users.noreply.github.com"` en config locale ;
verifier par `git log --format='%an <%ae>'` avant tout push.

## Fondations communes — ne pas recopier, completer a la source

- `bbl-hostrules` (0.5.0) : fiches, lecteurs (JSON, JSONC, TOML, YAML, Markdown, JSONL,
  dotenv ; SQLite decrit), masquage, resolution des chemins.
- `bbl-ui` : palette, briques iced, reglages, i18n. Meme visage que les autres outils.
- `bbl-report` : le contrat de la tour. `billfold report` produit un `bbl-report/1` ;
  chaque `Kind` a sa place sur les quatre gravites (`model.rs`), ecrite dans le CHANGELOG.
- Prix : `prices/apipriceindex.json`, instantane de `apipriceindex.com/api/all.json`
  (CC BY 4.0, attribution gardee), a rafraichir a chaque release ; les regles que l'index ne
  porte pas sont dans `providers/*.toml`.

## Reperes

- `crates/core/src/{model,sheets,providers,prices,wiring,keys,gitstat,scan,report,util}.rs`,
  `meter/<hote>.rs` (un lecteur par format) ; `hosts/*.toml` (21), `providers/*.toml` (22),
  schemas `docs/hosts.md` et `docs/providers.md`.
- Tests : `tests/promises.rs` (rien d'ecrit, rien de reseau, cle plantee jamais sortie,
  rapport conforme), `tests/scan.rs` (faux HOME, un constat de chaque sorte ;
  `BILLFOLD_FIXTURE_OUT=dir cargo test --test scan write_fixture` l'ecrit sur disque).
- `crates/cli` (`billfold`, `wiring`, `keys`, `meter`, `hosts`, `providers`, `report`),
  `crates/app` (`billfold-app --select <id> --tab keys|meter --since <jours>`).
- Les treize constats et leur gravite : `model.rs` et CHANGELOG.
- `docs/new-module.md` de la tour : la marche a suivre complete.
