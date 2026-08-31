# 📓 Historique des prompts — Onzer

> Journal de développement tenu automatiquement.
> Une entrée par requête : le prompt, le résumé de la réponse, les modifications concrètes, l'objectif.

---

## Entrée 001 — Cadrage du projet

**📅 Date :** 2026-08-31

**💬 Prompt**
Lancement d'une session de « vibecoding » pour créer Onzer, une application musicale
100 % hors-ligne (« Spotify local »). Règles de session imposées : architecture irréprochable
et scalable, UI/UX moderne en dark mode, code propre et modulaire, algorithmique avancée.
Progression strictement étape par étape avec validation à chaque palier, et proposition
d'options argumentées à chaque choix technique.
Fonctionnalités clés demandées : lecteur offline, moteur de recommandation IA local
(écoutes, skips, heure), statistiques poussées type Wrapped, et liaison à un script externe
pour l'import automatique de sons sans gestion manuelle des métadonnées.
Première demande : proposer une stack technique idéale et une structure de dossiers.

**🤖 Réponse (résumé)**
Analyse des contraintes techniques induites par chaque fonctionnalité, aboutissant au constat
qu'un backend natif est indispensable (l'analyse du signal audio pour la recommandation est
inatteignable en JavaScript). Trois stacks comparées avec avantages/inconvénients :
Tauri 2 + Rust (recommandée), Electron + Node, Flutter + Dart.
Proposition détaillée de la stack complète (crates Rust, librairies front), d'un aperçu du
moteur de recommandation hybride à 4 couches (contenu / comportement / contexte / exploration),
du pipeline d'import automatique à deux portes d'entrée, et d'une structure de dossiers
feature-sliced. Trois questions posées : stack, plateforme cible, versioning.

**🔧 Modifications**
Aucune — étape de proposition uniquement, conformément à la règle de validation préalable.

**🎯 Objectif**
Verrouiller les fondations techniques avant d'écrire la moindre ligne de code, afin d'éviter
des choix structurants coûteux à corriger plus tard.

---

## Entrée 002 — Validation de la stack et initialisation du dépôt

**📅 Date :** 2026-08-31

**💬 Prompt**
Réponses aux trois questions :
- Stack → **Tauri 2 + Rust + React**
- Plateforme → **macOS pour l'instant, avec un possible portage Android si le projet convainc**
- Versioning → **créer un nouveau dépôt GitHub**

**🤖 Réponse (résumé)**
Vérification de l'environnement de développement : Xcode, Git, Node 26 et GitHub CLI présents
et authentifiés ; **Rust manquant** et signalé comme prérequis à installer.
La cible « Android à terme » a été traduite en contraintes d'architecture concrètes et
consignées en ADR : décodage `symphonia` en pur Rust, sortie audio abstraite derrière un trait,
chemins de fichiers centralisés, cœur métier sans dépendance Tauri.
Point de vigilance identifié et neutralisé d'avance : `bliss-audio` dépend de FFmpeg,
difficile à cross-compiler pour Android — l'extraction de features est donc placée derrière
un trait `FeatureExtractor` interchangeable.

**🔧 Modifications**
- ➕ `README.md` — présentation du projet, stack, prérequis, commandes
- ➕ `.gitignore` — Rust / Node / Tauri / macOS, avec exclusion stricte des données utilisateur
- ➕ `docs/ARCHITECTURE.md` — ADR-001 à ADR-005 et feuille de route des décisions à venir
- ➕ `historique_prompts.md` — ce journal
- 🔧 Initialisation du dépôt Git local et création du dépôt GitHub distant

**🎯 Objectif**
Poser le socle documentaire et le versioning avant tout code, et transformer l'intention
« peut-être Android plus tard » en règles d'architecture contraignantes dès la première ligne,
seul moyen de rendre ce portage possible sans réécriture.

---

## Entrée 003 — Stratégie plateforme et stockage sur SSD externe

**📅 Date :** 2026-08-31

**💬 Prompt**
Précisions sur la bibliothèque : entre 500 et 5 000 morceaux, à stocker sur le SSD Lexar,
triés par auteur puis éventuellement par année ou par album. Rust confirmé comme installé.
Remise en question de la stratégie multi-plateforme : plutôt que du cross-platform,
faire une application macOS seule, puis une application Android distincte réutilisant les
mêmes concepts, et éventuellement Windows et iOS ensuite. Demande explicite de transparence
sur cette approche.

**🤖 Réponse (résumé)**
Approche validée, avec une auto-correction assumée : l'ADR-002 avait été présenté comme
« Android-ready » alors qu'aucune de ses cinq règles ne constituait un compromis pour Android.
Chacune a été rejustifiée par son seul bénéfice sur macOS — notamment `symphonia`, qui est
le seul moyen d'obtenir un gapless et un crossfade corrects. Distinction posée : ce qui casse
en cross-platform est le **partage de l'UI**, pas le partage du cœur métier.
Inspection de l'environnement : Rust 1.98 opérationnel, SSD Lexar de 954 Go **en exFAT**.
Quatre défauts d'exFAT documentés (pas de journalisation, insensibilité à la casse,
caractères interdits, verrous POSIX peu fiables) plus l'instabilité du point de montage,
menant à la décision de séparer l'audio (SSD) des données applicatives (disque interne).

**🔧 Modifications**
- ✏️ `docs/ARCHITECTURE.md` — ADR-002 **réécrit** : « Cœur métier isolé de l'UI » au lieu de
  « portable Android » ; chaque règle rejustifiée sur macOS ; droit explicite donné à la
  couche UI d'utiliser les API macOS (Now Playing, touches média)
- ➕ `docs/ARCHITECTURE.md` — **ADR-006** : audio sur le SSD, base SQLite et caches sur le
  disque interne ; règles induites (chemins relatifs, hash de contenu, `is_available`)
- ➕ `docs/ARCHITECTURE.md` — **ADR-007** : convention de rangement
  `Artiste/Année - Album/NN - Titre`, cas particuliers (singles, featurings, année inconnue,
  non identifiés), assainissement exFAT et journalisation des déplacements pour réversibilité

**🎯 Objectif**
Aligner l'architecture sur une cible macOS assumée sans sacrifier la réutilisabilité du cœur,
et neutraliser en amont les risques propres à une bibliothèque hébergée sur un volume exFAT
amovible — un problème qui, non traité, aurait provoqué des pertes de références de fichiers
et des corruptions de base.

---

## Entrée 004 — Schéma de base de données et socle technique

**📅 Date :** 2026-08-31

**💬 Prompt**
Réponses aux deux questions posées : dédoublonnage à l'import par **hash de fichier + tags**,
et pour la suite, consigne de faire « ce qui est recommandé pour mon cas ».

**🤖 Réponse (résumé)**
Conception du schéma autour d'un principe unique : `play_events` est un **journal
append-only**, et tous les compteurs et scores en sont dérivés, donc recalculables.
Ce choix garantit qu'une future évolution de la formule de recommandation pourra être
rejouée sur l'historique complet au lieu de repartir de zéro.
Six signaux comportementaux capturés dès la v1, dont `skip_at_ms` (position exacte du skip,
qui distingue un rejet d'un simple changement d'humeur) et `source` (origine de l'écoute,
seule façon de mesurer si le moteur est bon).
Le SQL a été validé par exécution réelle avant toute écriture de code Rust : FTS5 confirmé
disponible, recherche insensible aux accents vérifiée (`beyonce` → `Beyoncé`), et les quatre
garde-fous testés un à un.
Scaffolding complet du projet dans la foulée, avec la couche d'accès Rust et un écran de
diagnostic validant la chaîne React → IPC → Rust → SQLite de bout en bout.

**🔧 Modifications**
- ➕ `src-tauri/migrations/0001_initial.sql` — 17 tables réparties en 5 groupes
  (référentiel, journal, dérivés, playlists, système), index partiels, contraintes `CHECK`,
  table virtuelle FTS5 et trigger anti-suppression du journal
- ➕ `src-tauri/src/core/` — `error.rs` (erreur unique sérialisable pour l'IPC) et
  `paths.rs` (`PathResolver` : chemins relatifs, refus des remontées `..`, détection du
  SSD débranché) avec 7 tests
- ➕ `src-tauri/src/db/` — pool SQLite réglé (WAL, clés étrangères, cache 32 Mo),
  migrations embarquées dans le binaire, réglages persistants, 7 tests d'intégration
- ➕ `src-tauri/src/commands/system.rs` — commande `app_status`, sans logique métier (ADR-004)
- ➕ `src-tauri/src/lib.rs` + `main.rs` — démarrage, migrations bloquantes, restauration
  de la racine de bibliothèque, journalisation `tracing`
- ➕ Scaffolding frontend — Vite 7, React 19, TypeScript en mode strict maximal,
  Tailwind v4 avec jetons de design centralisés, client IPC typé, écran de diagnostic
- ➕ `docs/DB_SCHEMA.md` — justification de chaque décision structurante du schéma
- ✏️ `docs/ARCHITECTURE.md` — **ADR-008** (journal immuable) et section
  « Dette technique assumée » documentant le report de `specta` et l'absence d'icônes
- 🔧 Correction `tsconfig.node.json` — un projet référencé doit émettre (TS6306/TS6310)
- ➕ `.cargo/config.toml` — déplace les artefacts de compilation hors du volume exFAT
- ✏️ `src-tauri/build.rs` — purge les fichiers AppleDouble de `migrations/` avant
  l'expansion de `sqlx::migrate!`
- ➕ `tools/icon/generate-icon.mjs` — générateur d'icône sans dépendance (encodeur PNG
  écrit à la main), reprenant les jetons de design de l'application
- 🔧 Ajout de la feature `macros` à sqlx, sans laquelle `sqlx::migrate!` n'existe pas

**✅ Vérifications effectuées**
- 14 tests Rust au vert (résolution de chemins, migrations, FTS5, garde-fous du journal)
- Frontend compilé, TypeScript en mode strict sans erreur
- Application lancée réellement : base créée dans
  `~/Library/Application Support/com.loogatoxx.onzer/`, 24 tables, mode WAL confirmé,
  migration v1 appliquée, aucune erreur au démarrage

**🐞 Incident rencontré et résolu — les fichiers AppleDouble**
La première compilation a échoué : le build script de Tauri panique en lisant
`._default.toml` dans `target/`, un fichier binaire créé par macOS sur les volumes exFAT.
L'inspection a révélé un second piège, plus grave car silencieux : un
`._0001_initial.sql` présent dans `migrations/`, que `sqlx::migrate!` aurait sélectionné
comme une vraie migration puisqu'il filtre par extension `.sql`.
Deux parades posées : les artefacts de compilation sortent du volume exFAT (ce qui
accélère aussi nettement les builds), et `build.rs` nettoie `migrations/` à chaque
compilation. Les deux sont documentées dans l'ADR-006, faute de quoi ce code paraîtrait
arbitraire à la relecture.

**🎯 Objectif**
Rendre la base de données réellement opérationnelle et vérifiée, plutôt que validée sur le
papier. Chaque garde-fou a été testé en conditions réelles avant d'être documenté, et le
schéma capture dès maintenant les signaux comportementaux fins que le moteur de
recommandation exigera plus tard — ces données étant, par nature, impossibles à
reconstituer après coup.

---

## Entrée 005 — Politique de stockage et scanner de bibliothèque

**📅 Date :** 2026-08-31

**💬 Prompt**
Le SSD Lexar ne contient encore aucune musique. Consigne de continuer l'ensemble du
développement en réglant les problèmes au fur et à mesure, avec une contrainte forte :
**tout ce qui est lourd en stockage doit aller sur le SSD**, le disque interne du Mac
manquant de place. Pour le visuel, rester basique — le travail graphique sera repris
plus tard.

**🤖 Réponse (résumé)**
Audit de l'espace disque avant toute décision : 65 Go libres en interne, et surtout
**3,1 Go déjà consommés par le cache de compilation Rust** — contre 284 Ko pour la base.
L'ADR-006 avait donc fait le mauvais arbitrage en plaçant ce cache sur le disque interne.
Comme il ne peut pas non plus vivre directement sur le volume exFAT (les fichiers
AppleDouble font planter le build script de Tauri), la sortie retenue est une **image
disque APFS posée sur le Lexar** : stockage physique sur le SSD, système de fichiers natif.
Développement ensuite du scanner de bibliothèque complet : lecture des tags avec repli sur
le nom de fichier, rangement selon la convention, dédoublonnage en deux passes,
déplacement réversible, vignettes de pochettes et indexation plein texte.

**🔧 Modifications**
- ➕ `.onzer-build-cache.sparsebundle` (hors dépôt) + `tools/build-cache/ensure-mounted.sh`
  — cache de compilation déplacé sur le SSD, monté automatiquement par `npm run app`
- ➕ `src-tauri/src/library/naming.rs` — assainissement exFAT et construction des chemins
- ➕ `src-tauri/src/library/metadata.rs` — lecture des tags via lofty, repli sur le nom de
  fichier, séparation des featurings
- ➕ `src-tauri/src/library/hash.rs` — empreinte BLAKE3 par échantillonnage
- ➕ `src-tauri/src/library/artwork.rs` — vignettes 512 px mutualisées par album
- ➕ `src-tauri/src/library/importer.rs` — pipeline d'import, déplacement inter-volumes
  vérifié avant suppression de la source
- ➕ `src-tauri/src/library/scanner.rs` — parcours récursif, progression, contrôle de
  disponibilité
- ➕ `src-tauri/src/db/repository.rs` — upserts transactionnels, recherche FTS5, compteurs
- ➕ `src-tauri/src/commands/library.rs` — 7 commandes IPC + événement de progression
- ➕ `src/features/library/` — écran de configuration, liste, recherche, barre de
  progression, pochettes
- ✏️ `src-tauri/build.rs` — purge récursive des AppleDouble, généralisée à tout le crate
- ✏️ `docs/ARCHITECTURE.md` — **ADR-009** (politique de stockage) et **ADR-010**
  (pipeline d'import)

**🐞 Défaut de conception trouvé par les tests**
Le dédoublonnage par tags ne comparait que le titre et la durée. Deux albums différents
possédant chacun une piste « Intro » de durée voisine auraient été fusionnés à tort.
L'artiste a été ajouté au critère, et un test dédié verrouille désormais ce comportement.
Un second incident AppleDouble a par ailleurs frappé `capabilities/`, d'où la
généralisation de la purge dans `build.rs`.

**✅ Vérifications effectuées**
- **83 tests Rust au vert**, dont 15 tests de bout en bout fabriquant de vrais fichiers
  WAV tagués, les important, puis vérifiant à la fois le disque et la base
- Frontend compilé, TypeScript strict sans erreur
- Application lancée sans erreur, interface rendue sans erreur console

**🎯 Objectif**
Rendre la bibliothèque réellement alimentable, et corriger un arbitrage de stockage qui
aurait progressivement saturé le disque interne. La contrainte « le volumineux va sur le
SSD » est désormais inscrite en ADR plutôt que tenue de mémoire.

---
