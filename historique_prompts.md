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

---

## Entrée 006 — Moteur audio et journal d'écoute

**📅 Date :** 2026-08-31

**💬 Prompt**
Consigne de continuer avec l'étape recommandée, en respectant la structure, et d'ordonner
les fonctionnalités comme le ferait un vrai chef de projet — les deux features (moteur audio
et import automatique) devant de toute façon exister à terme.

**🤖 Réponse (résumé)**
Justification de l'ordre avant d'écrire la moindre ligne : le moteur audio est sur le
**chemin critique** de la recommandation et des statistiques, puisque `play_events` reste
vide sans lecture ; il porte le **risque technique le plus élevé**, qu'un chef de projet
attaque tôt ; et surtout, sa valeur **capitalise dans le temps** — chaque semaine sans
lecteur est une semaine de données d'écoute définitivement perdues pour l'IA. L'import
automatique, lui, aura la même valeur dans trois semaines qu'aujourd'hui.

Trois pièces distinctes, dont deux testables sans carte son :
le thread audio (qui possède le flux `cpal`, non `Send` sur macOS), la file de lecture, et
le journal d'écoute conçu comme une machine à états à **temps injecté**.

**🔧 Modifications**
- ➕ `src-tauri/src/audio/tracking.rs` — machine à états du journal d'écoute, 17 tests.
  Distingue temps écoulé, temps écouté et position ; capture la position exacte des sauts
- ➕ `src-tauri/src/audio/device.rs` — thread audio dédié, piloté par messages, état publié
  par entiers atomiques
- ➕ `src-tauri/src/audio/queue.rs` — file, répétition, aléatoire par Fisher-Yates, 18 tests
- ➕ `src-tauri/src/audio/mod.rs` — `PlayerService`, chef d'orchestre
- ➕ `src-tauri/src/audio/tests_playback.rs` — 5 tests de bout en bout ouvrant réellement le
  périphérique audio et décodant de vrais fichiers
- ➕ `src-tauri/src/db/events.rs` — persistance transactionnelle du journal, des compteurs,
  de la matrice de transitions et des totaux de session, 11 tests
- ➕ `src-tauri/src/commands/playback.rs` — 11 commandes IPC, deux canaux d'événements
- ➕ `src/features/player/` — barre de lecture complète et hook de synchronisation
- ✏️ `src/features/library/TrackList.tsx` — lignes cliquables, indicateur de lecture animé
- ✏️ `docs/ARCHITECTURE.md` — **ADR-011** (choix de rodio et sa porte de sortie) et
  **ADR-012** (journal d'écoute à temps injecté)

**🐞 Défaut trouvé par un test**
La position de lecture dérivait de quelques millisecondes **pendant les pauses** : `rodio`
rapporte l'écoulement du tampon de sortie, pas l'intention de l'utilisateur. L'horloge de
l'interface aurait tressauté à l'arrêt. La position est désormais figée explicitement en
pause, et republiée uniquement après une commande qui la déplace.

**✅ Vérifications effectuées**
- **132 tests au vert**, dont 5 qui ouvrent le vrai périphérique audio, décodent un fichier,
  attendent la fin du morceau et vérifient que l'écoute a été journalisée avec la bonne
  raison de fin
- Clippy sans aucun avertissement
- Application lancée : « moteur audio prêt » au démarrage

**🎯 Objectif**
Ouvrir la boucle de collecte de données le plus tôt possible. Le moteur de recommandation
n'existe pas encore, mais chaque écoute enregistrée à partir de maintenant l'alimentera —
et ces données sont, par nature, impossibles à reconstituer rétroactivement.

---

## Entrée 007 — Corrections du lecteur et pipeline d'import automatique

**📅 Date :** 2026-08-31

**💬 Prompt**
Le lecteur fonctionne, avec deux retours : le curseur de progression revient à sa position
d'origine au relâchement avant de sauter au bon endroit, et le mode « répéter un morceau »
semble se comporter comme une répétition normale. Feu vert pour enchaîner sur l'étape
suivante ensuite. Les remarques sur l'interface viendront plus tard.

**🤖 Réponse (résumé)**
Le retour du curseur avait une cause précise : la commande de déplacement est traitée de
façon **asynchrone** par le thread audio, si bien que l'instantané renvoyé à l'interface
contenait encore l'ancienne position. Corrigé des deux côtés — le thread publie la position
cible avant même d'envoyer la commande, et l'interface maintient la cible jusqu'à
confirmation.
Sur la répétition, le comportement était conforme à la convention (Spotify, Apple Music :
« suivant » change de morceau même en répétition unitaire), mais **invisible** : même icône
que la répétition totale, et la différence ne se manifeste qu'à la fin d'un titre. Le mode
a désormais sa propre icône, avec un 1 dans la boucle, et un libellé explicite.
Enchaînement ensuite sur le pipeline d'import automatique.

**🔧 Modifications**
- ✏️ `src-tauri/src/audio/device.rs` — publication immédiate de la position cible au
  déplacement
- ✏️ `src/features/player/PlayerBar.tsx` — maintien de la cible jusqu'à confirmation,
  avec filet de sécurité ; icône dédiée et libellés pour les trois modes de répétition
- ➕ `src-tauri/src/ingest/inbox.rs` — dossier surveillé par scrutin, avec détection de
  stabilité des fichiers, 11 tests
- ➕ `src-tauri/src/ingest/token.rs` — jeton d'API en `chmod 600`, comparaison en temps
  constant, 6 tests
- ➕ `src-tauri/src/ingest/server.rs` — API locale `axum` : `health`, `exists`, `import`
- ➕ `src-tauri/src/library/metadata.rs` — `MetadataHint` et son arbitrage face aux tags
- ➕ `docs/IMPORT_API.md` — contrat complet pour scripts externes
- ➕ `tools/downloader/onzer_client.py` — client Python sans dépendance, utilisable en
  ligne de commande ou comme module
- ✏️ `docs/ARCHITECTURE.md` — **ADR-013** (import automatique) et **ADR-014** (l'import
  range toujours)

**🐞 Trois défauts trouvés pendant les vérifications**
1. **Boucle de réimport infinie.** Un fichier dont l'import échoue reste dans le dépôt : il
   aurait été retenté toutes les six secondes, indéfiniment, saturant les journaux et la
   table `import_jobs`. Trouvé par un test.
2. **`GET /exists` ne trouvait rien.** La fonction phare de l'API était cassée : quand le
   script ne fournit pas de durée, une valeur factice de dix minutes était utilisée, et le
   critère de durée excluait alors tous les morceaux d'une autre longueur. La durée est
   désormais un `Option` réellement écarté quand elle est absente. Trouvé en interrogeant
   l'API sur un vrai morceau de la bibliothèque.
3. **Import laissé en vrac.** Un fichier déposé à la racine de la bibliothèque puis importé
   n'était pas rangé, la commande basculant en mode « indexer sans déplacer ». Constaté en
   observant la bibliothèque réelle. Corrigé par l'ADR-014.

**✅ Vérifications effectuées**
- **154 tests au vert**, clippy sans avertissement
- API testée en conditions réelles : refus sans jeton (401), refus d'un mauvais jeton,
  `health`, `exists` avec et sans durée, rejet d'un chemin relatif et d'un fichier absent,
  permissions `-rw-------` du fichier de jeton
- Dossier surveillé testé de bout en bout avec un vrai fichier audio : import automatique,
  artiste déduit du nom de fichier, rangement dans `Singles`. **Toutes les traces du test
  ont été supprimées** et la bibliothèque vérifiée identique à son état initial.

**🎯 Objectif**
Fermer la boucle d'alimentation de la bibliothèque : un script externe peut désormais
interroger Onzer avant de télécharger, puis lui remettre le fichier, sans jamais avoir à
connaître son fonctionnement interne.

