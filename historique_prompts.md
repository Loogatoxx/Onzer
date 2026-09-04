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

---

## Entrée 008 — Assemblage complet du moteur de recommandation

**📅 Date :** 2026-08-31

**💬 Prompt**
Validation de l'approche et feu vert en puissance maximale pour l'assemblage complet :
ouvrier d'analyse invisible pour l'utilisateur, générateur de playlists avec règles
strictes de diversité, et boutons « Radio » et « Pour toi maintenant » intégrés
naturellement au thème sombre. La maison doit être entièrement bâtie avant d'ajouter de la
musique à la main.

**🤖 Réponse (résumé)**
Assemblage des quatre fondations en un moteur fonctionnel : ouvrier d'analyse en tâche de
fond, score contextuel à lissage hiérarchique, règles de diversité dures et souples,
générateur pilotant six stratégies par échantillonnage de Thompson, et interface.
Trois défauts de conception trouvés en cours de route, dont deux qui n'étaient visibles
qu'à l'assemblage ou à la mesure.

**🔧 Modifications**
- ➕ `src-tauri/src/analysis/worker.rs` — ouvrier en tâche de fond, un morceau à la fois
  sur un fil bloquant, reprise après arrêt brutal, durée journalisée
- ➕ `src-tauri/src/reco/context.rs` — cinq périodes de la journée, lissage hiérarchique
  vers l'affinité globale, 17 tests
- ➕ `src-tauri/src/reco/diversity.rs` — règles dures et souples, 16 tests
- ➕ `src-tauri/src/reco/engine.rs` — chargement, notation par stratégie, assemblage,
  traçabilité et boucle d'apprentissage
- ➕ `src-tauri/src/reco/tests_engine.rs` — 13 tests d'assemblage sur bibliothèque synthétique
- ➕ `src-tauri/src/commands/reco.rs` — 6 commandes, génération suivie d'une lecture immédiate
- ➕ `src/features/discover/DiscoverBar.tsx` — pastilles « Pour toi maintenant » et
  « Tu les avais oubliés », jauge d'analyse qui disparaît une fois le travail fini
- ✏️ `src/features/library/` — bouton radio au survol de chaque ligne, bandeau de playlist,
  raison de présence affichée sous chaque morceau
- ✏️ `src-tauri/Cargo.toml` — profils de compilation corrigés
- ✏️ `docs/ARCHITECTURE.md` — **ADR-015** (ouvrier d'analyse) et **ADR-016** (quota adaptatif)

**🐞 Trois défauts trouvés en assemblant et en mesurant**
1. **Analyse 24,7 fois trop lente.** Mesurée à 6 591 ms par morceau, soit 9 heures pour
   5 000 titres. Cause : les dépendances DSP compilées sans optimisation en développement,
   et un profil de publication visant la taille du binaire plutôt que la vitesse. Après
   correction : **267 ms**, soit ~45 minutes pour 5 000 morceaux, résultat identique au bit
   près.
2. **Playlists mystérieusement courtes.** Un quota fixe de trois morceaux par artiste rend
   une playlist de vingt titres impossible dans une bibliothèque de six artistes. Le quota
   est désormais calculé pour rester réalisable. Invisible sur les modules isolés, trouvé
   par le test d'assemblage.
3. **Stratégies sans données gaspillant des emplacements.** Le bandit pouvait tirer
   « oubliés » ou « enchaînements » alors qu'ils n'avaient rien à proposer, amputant la
   playlist d'autant. Elles sont désormais écartées avant le tirage.

**✅ Vérifications effectuées**
- **300 tests au vert**, clippy sans avertissement
- Analyse validée sur le vrai fichier de la bibliothèque : **129,2 BPM** pour
  « Θ. Macarena » de Damso — valeur correcte — tonalité Ré majeur, vecteur de 54 dimensions
- Résultat identique après changement de profil de compilation : l'optimisation n'altère
  pas les valeurs produites
- Application lancée sans erreur : base, moteur audio, API d'import et ouvrier d'analyse
  tous opérationnels

**🎯 Objectif**
Rendre le moteur réellement utilisable, et surtout **mesurable** : chaque proposition est
tracée avec sa stratégie, chaque écoute renvoie un verdict, et les vues de qualité
permettront de vérifier que le moteur fait mieux que le hasard plutôt que de l'espérer.

---

> ⚠️ **Note de rattrapage.** Le journal s'est arrêté à l'entrée 008 pendant que le projet
> avançait de soixante-quatre commits. Les entrées 009 à 019 sont **reconstruites depuis
> l'historique Git** : les prompts y sont résumés de mémoire et non cités mot pour mot,
> contrairement aux précédentes et aux suivantes. La tenue du journal reprend au fil de
> l'eau à partir de l'entrée 020.

---

## Entrée 009 — Identification par empreinte et rétrospective d'écoute

**📅 Date :** 2026-08-31

**💬 Prompt**
Reconnaître automatiquement les morceaux mal étiquetés, et voir ses statistiques d'écoute.

**🤖 Réponse (résumé)**
Empreinte acoustique Chromaprint envoyée à AcoustID, puis MusicBrainz pour les métadonnées.
Clé d'API modifiable depuis les réglages. Page de statistiques conçue comme une
rétrospective : une section par écran, révélée au défilement.

**🔧 Modifications**
- ➕ `src-tauri/src/identify/` — empreinte, AcoustID, MusicBrainz
- ➕ `src/features/stats/WrappedView.tsx` — la rétrospective
- ✏️ Barre de lecture visible dès le lancement d'une playlist générée

**🎯 Objectif**
Réparer les métadonnées sans saisie manuelle, et donner à voir ce que la base sait déjà.

---

## Entrée 010 — Refonte complète de l'interface

**📅 Date :** 2026-08-31

**💬 Prompt**
L'interface ne tenait pas la comparaison. Tout revoir.

**🤖 Réponse (résumé)**
Charte assumée : monochrome, un seul accent, aucun dégradé sur de grandes surfaces,
typographie Avenir Next en poids d'affichage. En-tête de collection à grande pochette,
tableau à colonnes, panneau de lecture à droite.

**🔧 Modifications**
- ✏️ `src/styles/globals.css` — les jetons de design
- ➕ `src/components/PageHeader.tsx`, refonte de `TrackTable`
- 🐞 Doublons créés à chaque lancement, identifications erronées

**🎯 Objectif**
Que l'application ressemble à ce qu'elle prétend être.

---

## Entrée 011 — L'accueil, les ambiances, les artistes

**📅 Date :** 2026-09-01

**💬 Prompt**
Une page d'accueil qui propose, des catégories d'ambiance, une page par artiste.

**🤖 Réponse (résumé)**
Mix du jour et mix de goût calculés localement, catégories d'ambiance déduites des
descripteurs audio, page artiste avec ses albums. Menu de ligne complet, indicateur de
paroles.

**🔧 Modifications**
- ➕ `src/features/home/`, `src/features/artists/`, `src/features/discover/`
- 🐞 Le déplacement dans le morceau ne fonctionnait pas ; pochette de compilation restée

**🎯 Objectif**
Ouvrir l'application sur une proposition plutôt que sur une liste de deux mille lignes.

---

## Entrée 012 — Comparer une playlist, arbitrer les doublons, importer

**📅 Date :** 2026-09-01

**💬 Prompt**
Comparer une playlist Spotify à la bibliothèque pour voir ce qui manque, et récupérer
ce qui manque.

**🤖 Réponse (résumé)**
Comparaison **sans aucun service** (ADR-038) : le fichier exporté suffit. Doublons
arbitrables à l'écoute. Commande `spotdl` complète proposée à copier — Onzer ne télécharge
jamais lui-même et n'exécute jamais la commande qu'il affiche.

**🔧 Modifications**
- ➕ `src/features/library/DuplicatePanel.tsx`, `src/features/home/DiscoverPanel.tsx`
- ✏️ `docs/ARCHITECTURE.md` — ADR-033 révisé, ADR-034, ADR-035, ADR-038

**🎯 Objectif**
Combler les trous d'une bibliothèque sans dépendre d'une API fermée.

---

## Entrée 013 — Pochettes, paroles, correction manuelle

**📅 Date :** 2026-09-01

**💬 Prompt**
Des pochettes pour tout, des paroles lisibles, et pouvoir corriger à la main.

**🤖 Réponse (résumé)**
Pochettes Deezer passées par Rust en `data:` — la politique de sécurité du webview reste
fermée. Paroles en pleine largeur. Dialogue de correction et « chercher ailleurs ».

**🔧 Modifications**
- ➕ `CorrectDialog`, `MatchDialog`, `LyricsView`
- 🐞 La recherche bloquait la navigation ; la bibliothèque s'arrêtait à 500 morceaux

**🎯 Objectif**
Qu'aucun morceau ne reste orphelin d'image, de texte ou de titre juste.

---

## Entrée 014 — Les paroles qui défilent

**📅 Date :** 2026-09-02

**💬 Prompt**
Des paroles calées sur la musique.

**🤖 Réponse (résumé)**
Trois sources par ordre de coût : celles déjà présentes dans les fichiers, les `.lrc` d'un
dépôt public, puis un modèle local (`whisper.cpp`) pour caler à l'oreille — à la demande,
morceau par morceau. Interrupteurs séparés pour la complétion en ligne et l'identification
automatique.

**🔧 Modifications**
- ➕ `src-tauri/src/lyrics/`, `src/features/lyrics/`
- 🐞 Les paroles non synchronisées étaient comptées comme faites

**🎯 Objectif**
Lire en même temps qu'on écoute, sans rien envoyer qu'on n'ait choisi d'envoyer.

---

## Entrée 015 — Robustesse et pagination

**📅 Date :** 2026-09-02

**💬 Prompt**
L'accueil reste bloqué, la bibliothèque rame, le cache de compilation casse.

**🤖 Réponse (résumé)**
Bibliothèque paginée par cent, triée **en base**. Cache de compilation qui se répare seul.
Un morceau ayant perdu son fichier n'est plus traité comme un doublon.

**🔧 Modifications**
- ➕ `src/features/library/Pager.tsx`
- ✏️ Tri déplacé côté SQL ; compteurs d'en-tête recalculés après réparation

**🎯 Objectif**
Tenir la charge de deux mille morceaux sans que rien ne se fige.

---

## Entrée 016 — Le portage Android

**📅 Date :** 2026-09-02

**💬 Prompt**
Faire tourner Onzer sur le téléphone.

**🤖 Réponse (résumé)**
`tauri android init`, chaîne NDK, empaquetage. L'écran de premier lancement sait sur quelle
machine il tourne. Accès aux fichiers du téléphone.

**🔧 Modifications**
- ➕ `src-tauri/gen/android/` — projet Android (versionné seulement à l'entrée 023)
- ➕ `src-tauri/src/android.rs`

**🎯 Objectif**
Le même cœur des deux côtés, sans réécriture.

---

## Entrée 017 — L'interface mobile

**📅 Date :** 2026-09-02 → 2026-09-03

**💬 Prompt**
Rendre l'application utilisable au pouce, avec la lecture en arrière-plan.

**🤖 Réponse (résumé)**
Barre d'onglets en bas, écran de lecture plein écran, en-têtes empilés. Session média
Android : la lecture apparaît sur l'écran verrouillé, via un service de premier plan.

**🔧 Modifications**
- ➕ `PlaybackService.kt`, `MiniPlayer`, `MobileTabs`, `MoreView`, `NowPlayingView`
- 🐞 Lancer un morceau fermait l'application (règles `-keep` de R8 manquantes)
- 🐞 Les gestes tactiles se perdaient en silence (`pointercancel` au défilement)

**🎯 Objectif**
Un vrai lecteur de téléphone, pas une fenêtre de bureau rétrécie.

---

## Entrée 018 — Le Mac et le téléphone se mettent d'accord

**📅 Date :** 2026-09-03

**💬 Prompt**
Faire comme un compte en ligne : synchroniser le téléphone et le Mac par Wi-Fi, avec un
QR code ou un lien.

**🤖 Réponse (résumé)**
Appairage local par QR et code à huit chiffres, serveur HTTP éphémère sur le réseau, fusion
pure et testée des deux états (favoris, playlists, écoutes). Transfert des fichiers manquants
à la demande. Aucun compte, aucun serveur distant.

**🔧 Modifications**
- ➕ `src-tauri/src/sync/` — `fusion.rs`, `appairage.rs`, `client.rs`
- ➕ `src/features/appairage/` — appairage et scanner QR intégré
- 🐞 187 faux « manquants » ramenés à 83 par une troisième clé de rapprochement
- 🐞 Les onze doublons reproposés à chaque synchronisation (table `sync_alias`)

**🎯 Objectif**
Deux bibliothèques d'accord, sans confier quoi que ce soit à personne.

---

## Entrée 019 — La file, le repérage, la sélection, la reprise

**📅 Date :** 2026-09-03

**💬 Prompt**
Pouvoir ajouter des playlists et des sons dans une liste d'attente sur les deux versions —
puis, parmi une liste de quatorze propositions : « fais dans l'ordre que tu veux mais je
trouve tout incroyable ».

**🤖 Réponse (résumé)**
Quatre chantiers : une file d'attente qu'on réordonne au doigt, le repérage (aller au
morceau en cours, récemment joué, hors ligne), la sélection multiple, et la reprise de
l'écoute sur l'autre appareil. Au passage, un défaut plus ancien : « À suivre » montrait
l'ordre d'origine et non l'ordre de lecture — il mentait dès que l'aléatoire était allumé.

**🔧 Modifications**
- ✏️ `src-tauri/src/audio/queue.rs` — `insert_next`, `remove_at`, `move_position`
- ➕ `QueueView`, `BarreSelection`
- ✏️ `docs/ARCHITECTURE.md` — ADR-088, ADR-089

**🎯 Objectif**
Tenir sa file comme on tient une pile de disques.

---

## Entrée 020 — Les captures Spotify, et la grande liste

**📅 Date :** 2026-09-03

**💬 Prompt**
« voici les screen, j'ai juste pas la feature premium donc je peut pas montrer comment ils
font le lecteur sur mobile. sinon le glissement marche très bien merci »
(quinze captures : Spotify Mac et mobile)

**🤖 Réponse (résumé)**
Quatorze relevés, classés en quatre thèmes, en écartant ce qui n'a pas de sens hors ligne
(radios, recommandations, podcasts, Jam, Blend, auditeurs mensuels). Constat principal :
**la structure y est déjà**. Un seul vrai défaut trouvé — sur téléphone, aucun chemin ne
menait d'un morceau à son album.

**🔧 Modifications**
Aucune : liste soumise à validation.

**🎯 Objectif**
Comparer sans copier, et ne retenir que ce qu'un lecteur local peut faire mieux.

---

## Entrée 021 — La passe de finition

**📅 Date :** 2026-09-03

**💬 Prompt**
« as tu quelque chose de pertinent a modifier avec les screen […] verifie le fameux intuition
humaine ou parfois on a envie d'appuyer sur des endroit pour ouvrir des truc au final y a
rien de programmer, et les petit detail couleur etc […] les bouton bleu degrader dans
sychro ça fait un peu IA essaye de garder une couleur unique […] les animation fait des truc
plus propre comme les pro font »

**🤖 Réponse (résumé)**
Trois audits menés en parallèle par des sous-agents (affordances mortes, dégradés, animations),
puis trois chantiers. **Mesures** : le fondu de l'en-tête ne traverse que quinze niveaux, soit
83 lignes unies sur 250 — d'où le grain. La courbe de mouvement maison ne régissait que huit
animations sur cent cinquante et une ; `.pression`, hors couche CSS, **écrasait** le
`transition-colors` de dix-sept éléments. Une soixantaine d'endroits où la main partait dans
le vide.

**🔧 Modifications**
- ✏️ `globals.css` — grain tramé, bouton d'accent plat, courbe par défaut, sorties animées
- ✏️ Vingt-cinq cibles tactiles dotées d'un retour d'appui ; file d'attente qui se pose ;
  glissement élastique ; grand lecteur qui suit le doigt
- ✏️ Une vingtaine d'affordances mortes branchées (artiste, album, compteurs, rétrospective)
- ✏️ `docs/ARCHITECTURE.md` — ADR-090, ADR-091, ADR-092
- ➖ `--color-accent-alt` (le cyan), sans usage hors dégradé

**🎯 Objectif**
Que l'interface réponde partout où elle en a l'air, et qu'elle bouge d'une seule main.

---

## Entrée 022 — Les régressions, et la barre noire

**📅 Date :** 2026-09-03

**💬 Prompt**
« les numero des titre son fix et ne subissent pas le scroll », le geste retour qui quitte
l'application depuis « Plus » au lieu de passer par l'accueil, « on vois une barre noir a
droite qui n'est pas de la meme couleur », et « avec la luminositer basse on voit moin bien
l'escalier mais quand je met la luminositer normal c'est flagrant ».

**🤖 Réponse (résumé)**
Quatre défauts, tous mesurés sur l'appareil.
1. **Les numéros** : le `relative` d'ancrage n'avait jamais été appliqué — mon motif de
   remplacement avait deux espaces d'indentation de trop, et c'était la seule substitution
   de la passe sans vérification. Les éléments absolus se plaçaient donc par rapport à la page.
2. **Le geste retour** : un navigateur marque « à sauter » toute entrée d'historique empilée
   sans geste de l'utilisateur. Nos entrées reposées pendant le retour étaient enjambées.
   Elles sont désormais empilées par `navigate`, juste après l'appui.
3. **La barre noire** : 32 pixels physiques constants à (18,18,20) — le couloir réservé par
   `::-webkit-scrollbar`, supprimé sous 768 px.
4. **L'escalier** : la capture ne dit rien de la luminosité de la dalle. Fondu écourté aux
   trois cinquièmes et grain porté à 5,5 % en bruit d'une seule octave — 10 lignes unies sur
   264, contre 153 sur 440 sans rien.

**🔧 Modifications**
- ✏️ `TrackTable.tsx`, `AppShell.tsx`, `globals.css`, `PageHeader.tsx`
- ✏️ `docs/ARCHITECTURE.md` — ADR-090 complété

**🎯 Objectif**
Réparer ce que j'avais cassé, et ce que la mesure seule ne voyait pas.

---

## Entrée 023 — Trois manques, et le dépôt remis à jour

**📅 Date :** 2026-09-03

**💬 Prompt**
« applique ce que tu me conseil et on voit ce que ça donne », puis « je me demande si tu
continue a alimenter github oublie pas […] faut aussi si c'est pas fait ajouter la version
mobile », puis « va falloir reprendre en main tout ça c'est ecrit dans les skill ».

**🤖 Réponse (résumé)**
Les trois relevés chez Spotify qui manquaient vraiment : chercher dans la liste ouverte,
la colonne « Lectures » (le compteur existait en base depuis le premier jour, affiché nulle
part), et la feuille du bas au pouce sur téléphone. Puis **sept commits poussés** sur
GitHub, et surtout : le projet Android n'était pas versionné du tout — un clone frais aurait
perdu le service de lecture, les règles `-keep` et les permissions, en silence.

**🔧 Modifications**
- ✏️ `repository.rs` — `play_count` dans la projection commune, `SortColumn::Plays`
- ➕ Filtre de liste dans `PageHeader`, feuille du bas dans `TrackTable`
- ✏️ `.gitignore` — `src-tauri/gen/android/` entre dans le dépôt (1 297 lignes)
- ➕ Reprise de ce journal, à l'arrêt depuis l'entrée 008

**🎯 Objectif**
Rendre au dépôt ce qui n'existait que sur cette machine, et reprendre la tenue du journal.

---

## Entrée 024 — L'escalier ne venait pas du dithering

**📅 Date :** 2026-09-03

**💬 Prompt**
« y a plus le trait noir mais je vois encore l'effet escalier je sais pas si tu vois »
(après : « avec la luminositer basse on voit moin bien l'escalier mais quand je met la
luminositer normal c'est flagrant »)

**🤖 Réponse (résumé)**
Mesure faite sur une capture de son appareil, dans la zone du fondu : **zéro ligne unie sur
six cents**, contre 23 avant la dernière passe. Il n'y a plus une seule bande à trouver dans
le tampon d'image — et l'escalier reste visible à l'œil.

Le dithering est donc au bout de ce qu'il peut faire, et le défaut n'est pas dans nos
valeurs : ce que l'écran en fait après nous — profil de couleur, gestion des noirs, PWM —
nous échappe, et aucune trame ne survit à une seconde quantification qu'on ne contrôle pas.

L'en-tête devient un **aplat**. Un aplat n'a rien à quantifier. C'était d'ailleurs la règle
écrite en tête de la feuille de style depuis le premier jour — aucun dégradé sur de grandes
surfaces — et je l'avais contournée en croyant pouvoir tramer.

**🔧 Modifications**
- ✏️ `src/styles/globals.css` — `.fondu-tete` devient une couleur unie ; l'historique des
  trois tentatives est conservé en commentaire
- ✏️ `src/components/PageHeader.tsx` — le grain retiré de l'en-tête : appliqué à un aplat,
  il **est** le dégradé, puisque son masque s'éteint vers le bas
- Le grain reste sur les halos de la rétrospective et de l'écran de chargement, où
  l'ambiance est le propos

**🎯 Objectif**
Arrêter de raffiner un effet que la dalle défait, et assumer une limite franche : une
frontière assumée se lit comme une frontière, une bande ratée se lit comme un défaut.

---

## Entrée 025 — L'escalier venait de l'ombre

**📅 Date :** 2026-09-03

**💬 Prompt**
« peut etre mais je vois aussi une sorte d'escalier avec l'ombre derrière la grosse icône »

**🤖 Réponse (résumé)**
Mesure décisive sur sa capture, en profil horizontal à droite de la grande pochette :
**des paliers de 7 à 12 pixels**, séparés d'un seul niveau de luminance, sur 130 pixels.
Ce sont les anneaux de `shadow-2xl shadow-black/50`.

Une ombre portée est un dégradé noir de cinquante pixels de flou. Posée sur un fond déjà
quasi noir, elle ne peut traverser que trois ou quatre niveaux : elle dessine des anneaux
au lieu d'assombrir. **Une ombre a besoin de clair pour travailler** ; ici il n'y en avait
pas — elle ne portait rien et coûtait ses artefacts.

L'audit couleur du début de soirée l'avait signalé — « dix-sept ombres noires sur du
quasi-noir, dont le fondu participe au même effet » — et je l'avais écarté pour me
concentrer sur les dégradés déclarés. C'était l'essentiel du défaut.

**🔧 Modifications**
- ➖ `shadow-2xl shadow-black/50` sur la pochette de `PageHeader` et sur celle de l'écran
  de lecture : elles se détachent par leur coin arrondi et par leur propre image
- ✏️ Pastilles de lecture de l'accueil et des artistes : `shadow-xl` → `shadow-lg` — posées
  sur une pochette, elles ont de quoi assombrir, mais moins large
- Les ombres des panneaux flottants restent : elles surplombent du contenu, pas du noir

**🎯 Objectif**
Chercher le défaut là où il est plutôt que là où on l'a supposé. Trois passes sur le fondu
n'auraient jamais rien donné : ce n'était pas lui.

---

## Entrée 026 — Un aplat qui ne se coupe plus, et la couleur des pochettes

**📅 Date :** 2026-09-03

**💬 Prompt**
« prochaine etape fait les deux que tu vien de dire et la c'est devenu un peu moche avec une
ligne qui s arrete net dans la biblioteque y a meme plus de fondu ta juste suprimer tout
essaye de trouver une alternative plus belle sois tu revien sur du fondu mais bien gerer sois
autre chose mais de plus coherent et artistique »

**🤖 Réponse (résumé)**
Une bande est une amplitude divisée par une distance. L'amplitude est
intouchable — neuf niveaux entre le voile et la page. Restait la distance : le voile est plein
sur toute sa hauteur et ne se dissout que sur ses **trente-deux derniers pixels**. Une marche
tous les trois pixels et demi, soit deux dixièmes de millimètre.

Pour la part « artistique », la charte du projet disait déjà, depuis le premier jour, que « les
couleurs de l'écran viennent des albums, pas du châssis ». Le voile prend maintenant la
**teinte** de la pochette — et seulement sa teinte : la luminosité et la saturation sont
imposées, sans quoi une pochette blanche donnerait du blanc et le titre deviendrait illisible.

**🔧 Modifications**
- ➕ `src-tauri/src/library/teinte.rs` — vote de teinte par secteurs, huit tests
- ➕ commande `artwork_tint`, avec cache par empreinte
- ➕ `src/lib/useTeinte.ts` ; `PageHeader`, bande d'encoche et sélecteur d'onglets teintés
- ✏️ `.fondu-tete` : aplat dissous sur 2 rem

**🎯 Objectif**
Sortir du faux choix entre un dégradé qui fait des marches et un aplat qui coupe net.

---

## Entrée 027 — Le menu d'une ligne sautait, et lançait des morceaux

**📅 Date :** 2026-09-03

**💬 Prompt**
« quand j appuye sur les trois petit point des titres l animation est un peu buggé vraiment pas
fluide » puis « quand j appuye a cote ça ppuye derriere et ça peu lancre une autre musique »

**🤖 Réponse (résumé)**
Deux défauts, une seule cause de fond.

La ligne portait `.pression`, qui lui applique une mise à l'échelle pendant l'appui. Or **un
élément `fixed` placé dans un ancêtre transformé se positionne par rapport à lui**, pas par
rapport à l'écran : le menu, ouvert au bout de l'appui long, naissait dans une ligne encore
enfoncée puis sautait quand le doigt se levait.

Le voile, lui, fermait au clic — qui n'arrive qu'après le relâchement, quand la feuille a déjà
commencé sa sortie et que l'animation de sortie coupe les événements. Le clic tombait sur la
ligne du dessous.

**🔧 Modifications**
- ✏️ La feuille est portée à la racine du document (`createPortal`)
- ✏️ Le voile ferme au `pointerdown` et empêche le clic de naître ; il garde ses clics pendant
  qu'il s'efface
- ✏️ Les lignes s'éclairent au lieu de se mettre à l'échelle — c'est ce que font les listes
  natives, et cela ne piège rien

**🎯 Objectif**
Qu'un menu s'ouvre là où on l'a demandé, et qu'en le fermant on ne lance rien.

---

## Entrée 028 — La liaison continue

**📅 Date :** 2026-09-03

**💬 Prompt**
« prochaine etape fait les deux que tu vien de dire » (synchronisation continue + reste de la
liste Spotify), puis « quand tout sera setup tu testera la syncro continue par toi meme ».

**🤖 Réponse (résumé)**
Deux appareils avec chacun leurs haut-parleurs ne peuvent pas jouer ensemble : un seul tient le
son, l'autre le voit et le commande. Une requête `GET` qui ne répond pas tout de suite remplace
le WebSocket — même latence, aucune dépendance de plus à compiler pour quatre architectures.

Une seule boucle des deux côtés ; seul le chemin d'accès au tableau change.

**🔧 Modifications**
- ➕ `src-tauri/src/sync/continu.rs` — le raisonnement pur, quatorze tests
- ➕ `src-tauri/src/sync/liaison.rs` — le transport et la boucle, quatre tests
- ➕ Routes `GET`/`POST /sync/v1/continu` ; commandes `link_command`, `link_open`, `stop_link`
- ➕ `src/features/appairage/Telecommande.tsx`
- ➕ Ordre d'origine dans le menu de tri ; bouton « + » dans l'en-tête mobile

**🐞 Trois défauts trouvés par les tests, avant la moindre exécution**
1. `watch::Sender::send` **échoue quand personne n'écoute** : le tableau ne se mettait jamais à
   jour tant qu'aucune attente n'était en cours.
2. Sans numéro d'ordre, le tableau relu en boucle rejouerait la même pause à l'infini.
3. Deux appareils arrêtés chacun sur un morceau se disputaient le tableau à chaque seconde.

**🎯 Objectif**
Que le téléphone serve de télécommande au Mac, et réciproquement, sans compte ni serveur.

---

## Entrée 029 — Un seul gris, et une feuille qu'on renvoie du doigt

**📅 Date :** 2026-09-04

**💬 Prompt**
« retire ce genre de orange beige finalement fait comme sur toute les autre page reste sur le
meme gris meme dans la biblioteque de haut en bas, et pour le menu des 3 petits point sur les
musique fait en sorte que en puisse slide vers le bas en prenant le haut du menu comme pour le
ranger et ça le ferme en suivant le doigt qui slide »

**🤖 Réponse (résumé)**
Le voile de l'en-tête disparaît entièrement — teinte comprise, et avec elle toute la plomberie
qui l'alimentait. La bibliothèque garde le même gris que l'accueil, les artistes ou les
réglages, du haut de l'écran au bas. Ce qui distingue un en-tête, ce sont son titre en quatre
rem et sa pochette, pas un voile.

Quatre tentatives auront été nécessaires pour arriver là : fondu, fondu tramé, aplat dissous,
aplat teinté. La bonne réponse était de n'en mettre aucun — et c'est l'usage qui l'a dit, pas la
mesure.

La feuille du menu se renvoie maintenant vers le bas : on la prend par le haut, elle suit le
doigt sans transition, le voile pâlit à mesure, et au-delà de quatre-vingt-dix pixels elle
achève sa descente. Une poignée dessinée le dit — un geste sans prise n'existe pas.

**🔧 Modifications**
- ➖ `src-tauri/src/library/teinte.rs`, la commande `artwork_tint`, `src/lib/useTeinte.ts`
- ✏️ `globals.css`, `PageHeader`, `AppShell` — plus de bande, ni sur l'encoche ni sur les onglets
- ✏️ `TrackTable` — poignée, suivi du doigt, seuil de renvoi

**🎯 Objectif**
Une seule surface, et un menu qu'on referme du geste qu'on fait partout ailleurs.

---

## Entrée 030 — Choisir ce qui traverse, et une image qui passe enfin

**📅 Date :** 2026-09-04

**💬 Prompt**
« je ne met plus mon telephone a disposition pour le moment […] fait des amelioration, pouvoir
selectionner se que on veut synchro (j aime, playlist, musique autre ou tout, et si on coche
pour les musique que on puisse choisir quel musique ou toute ou un artiste en particulier) et
regler le probleme du fait de mettre une image pour playlist "image ilisible : no such file or
directory (os error 2)" sinon le bout en bout marche nickel »

**🤖 Réponse (résumé)**

**L'image.** La commande recevait un **chemin de fichier**. Sur un téléphone il n'y en a pas :
le sélecteur d'Android rend un `content://`, une adresse que seul le résolveur de contenu du
système sait ouvrir. `fs::read` échouait donc sur une image parfaitement présente, et le message
accusait le fichier plutôt que notre lecture. L'image traverse maintenant en octets, comme les
pochettes le font déjà dans l'autre sens — aucun chemin, aucune permission, aucune sémantique de
système de fichiers. Elle est réduite à 1 024 pixels avant de partir : le cœur n'en garde qu'une
vignette de 512, et une photo de téléphone pèse vingt mégaoctets.

**La portée.** Quatre cases — J'aime, Playlists, Morceaux manquants, Paroles et reprise — plus
un champ d'artiste quand les morceaux traversent. Tout est coché d'avance : c'est ce qu'on veut
la plupart du temps, et un réglage qui commence à moitié coché oblige à comprendre avant de
pouvoir s'en servir.

La portée **voyage avec la demande** : les deux appareils fusionnent chacun de leur côté, et si
seul le demandeur la respectait, l'autre appliquerait quand même tout — deux bibliothèques qui
cesseraient de dire la même chose, ce que la synchronisation est censée empêcher. Elle est
`default` à la désérialisation, donc un appareil qui n'a pas encore la mise à jour continue de
tout échanger, comme il l'a toujours fait.

**🔧 Modifications**
- ➕ `Portee` dans `sync/fusion.rs`, avec sept tests ; comparaison d'artiste sans accents ni casse
- ✏️ `fusionner` prend la portée ; la route et le client la font voyager à plat (`flatten`)
- ➕ `ChoixPortee` dans `PairingView` ; champ d'artiste avec la liste de ceux qu'on possède
- ➕ `decode_base64` dans `library/artwork.rs`, avec ses tests d'aller-retour
- ➕ `src/lib/choisirImage.ts` — champ de fichier, réduction à 1 024 px
- ✏️ `set_playlist_cover` reçoit des octets

**🎯 Objectif**
Pouvoir ne prendre qu'une partie, et pouvoir donner une image à une playlist depuis le
téléphone.

---

## Entrée 031 — Le test de bout en bout, et ce qu'il a trouvé

**📅 Date :** 2026-09-04

**💬 Prompt**
« tout est pret tu peu telecharger et test », puis « le ajouter une description n est pas centrer
sur la playlist telephone »

**🤖 Réponse (résumé)**
Test complet, les deux appareils en main. Ce qui marche, vérifié à l'écran :

- la **portée** — décocher « Playlists » décoche « Tout » et le résumé devient « J'aime ·
  Morceaux · Paroles » ;
- la **synchronisation** avec la portée : 68 morceaux manquent au Mac, 161 ici, aucune erreur ;
- la **colonne Lectures** dans la bibliothèque du Mac ;
- le **gris uniforme** : une seule teinte sur 1 400 pixels, mesurée.

Et deux défauts que seul un appareil pouvait révéler.

**🐞 La liaison mourait en quittant l'écran**
La porte ouverte, le téléphone connecté, la musique lancée — et la télécommande n'apparaissait
jamais. Le tableau ne répondait même plus : connexion refusée en une milliseconde. La porte se
referme en quittant l'écran de synchronisation, c'est sa règle de sûreté ; mais la liaison
continue vit **dans** la porte. Aller à la bibliothèque pour lancer un morceau la coupait. On
n'avait donc de télécommande qu'en regardant l'écran de synchronisation, c'est-à-dire au seul
moment où l'on n'écoute pas.

Aucun test unitaire ne pouvait le trouver : chaque moitié était juste, c'est leur rencontre avec
le cycle de vie d'un écran qui ne l'était pas. La porte reste désormais ouverte tant qu'une
liaison la traverse — et il faut qu'un pair soit venu, code en main, pour qu'elle le fasse.

**🐞 Le sélecteur d'image ne s'ouvrait pas**
La pochette touchée : rien. `onShowFileChooser` n'était jamais appelé. Un `input` **détaché** du
document ouvre bien le sélecteur sur un navigateur de bureau — ce qui rendait le défaut
invisible en développement — mais la WebView d'Android ne voit que ce qui est dans le document.

**🔧 Modifications**
- ✏️ `close_pairing` respecte une liaison établie ; commande `end_link` et bandeau « Liaison
  ouverte — Couper »
- ✏️ La boucle du client renonce après quarante tentatives espacées, au lieu d'interroger le vide
  toute la nuit
- ✏️ `choisirImage` insère le champ dans le document ; délai d'annulation porté à 1,5 s
- ✏️ « Changer l'image » visible au doigt ; « Ajouter une description » centrée en étroit

**🎯 Objectif**
Éprouver ce qui n'avait jamais tourné entre deux vraies machines, et corriger ce que cela
révèle.

---

## Entrée 032 — Un titre précis, et des coins propres

**📅 Date :** 2026-09-04

**💬 Prompt**
« l affichage du selecteur d artiste est un peu mal fait avec les arrondi dans les coin on vois
les separateur en ligne qui depasse sur les coin arrondi, et y a pas de selecteur de titre precis
imagine si y a une seul musique que on veut transmettre parce que les autres on veut pas sinon
continue sur la prochaine etape fait tout toi meme »

**🤖 Réponse (résumé)**
Les séparateurs internes traversent toute la largeur du bloc ; sans rognage, ils dépassent des
angles arrondis. `overflow-hidden` les coupe à la courbe.

Un champ de titre s'ajoute à celui de l'artiste, avec des suggestions cherchées à la frappe —
deux mille trois cents morceaux ne se versent pas dans une liste pour en montrer dix. Les deux se
cumulent : un titre porté par deux artistes se départage en nommant l'artiste, et l'on n'a jamais
à choisir entre préciser trop et pas assez.

**🔧 Modifications**
- ✏️ `Portee` gagne `titre` ; le filtre passe par une fonction commune aux deux champs
- ➕ Deux tests : un titre visé ne laisse passer que lui ; artiste et titre se cumulent
- ✏️ `ChoixPortee` : champ de titre avec recherche à la frappe, résumé qui l'affiche entre
  guillemets, `overflow-hidden` sur le bloc

**🎯 Objectif**
Pouvoir ne donner qu'un morceau, et que la boîte qui le demande soit bien dessinée.


---

## Entrée 033 — Un journal, parce qu'on ne corrige pas ce qu'on ne voit pas

**💬 Prompt**
« quand tout sera setup tu testera la syncro continue par toi meme » (suite) — et
« je suis revenu si il y a un probleme dis »

**🤖 Réponse (résumé)**
L'ordre déposé sur le tableau n'était jamais exécuté par le Mac, et rien ne disait pourquoi :
chaque échec du chemin — pas de lecteur audio, ordre refusé — ne laissait qu'un `warn!` sur la
sortie standard, que macOS relie au néant pour une application lancée depuis le Finder. Le défaut
était invisible par construction.

Deux tests neufs suivent l'ordre du réseau jusqu'au lecteur : la boucle complète, puis le
scénario réel — une requête HTTP pose un ordre, la porte l'écrit, la boucle s'en saisit. Les deux
passent. Le chemin est donc sain de bout en bout **sauf** son dernier pas, celui qui touche le
lecteur, et que seule une machine réelle peut éprouver.

D'où le journal sur disque : `~/Library/Logs/Onzer/onzer.log` sur Mac, le dossier temporaire
ailleurs. Il s'ajoute à la sortie standard, il repart à zéro passé quatre mégaoctets en gardant
le fichier précédent, et il porte enfin ce qui manquait — l'ordre reçu, l'ordre exécuté, la
raison d'un refus.

**🔧 Modifications**
- ➕ `init_tracing` écrit aussi dans un fichier ; `ouvrir_journal` et `dossier_journaux`
- ➕ Test : la boucle obéit à un ordre déposé
- ➕ Test : un ordre venu du réseau est exécuté (porte HTTP réelle + boucle réelle)
- ✏️ Traces sur le chemin des ordres : tableau reçu, ordre reçu, ordre exécuté, ordre sans lecteur
- ✏️ `link_open` demande aussi qu'un pair soit venu — une boucle qui attend seule n'est pas une
  liaison
- ✏️ Le client note le pair quand la porte d'en face répond
- ✏️ Les tests qui ouvrent la porte partagent le verrou du tableau : ils se fermaient la porte
  les uns aux autres

**🎯 Objectif**
Voir ce qui se passe chez l'utilisateur, au lieu de le deviner.

---

## Entrée 034 — La télécommande, éprouvée sur l'appareil

**💬 Prompt**
« c'est bon j ai lancer une musique » puis « la porte du tel est 52449419 »

**🤖 Réponse (résumé)**
La liaison continue marche, de bout en bout, sur du vrai matériel. Le Mac a piloté le téléphone
par le réseau, et le tableau a suivi chaque ordre :

```
état initial    : Siliconé — Niska | 42605ms | lecture=False
après lecture   : Siliconé — Niska | 46550ms | lecture=True
3 s plus tard   : Siliconé — Niska | 49570ms | lecture=True
après pause     : Siliconé — Niska | 50595ms | lecture=False
après suivant   : Chasse à l'homme — Niska | 1945ms | lecture=True
après précédent : Siliconé — Niska | 1895ms | lecture=True
```

Le journal du Mac a d'abord montré que le premier essai échouait pour une raison bête : aucun
morceau chargé, et `toggle` sur un lecteur vide réussit sans rien faire.

En insistant avec un code périmé, la porte du Mac s'est refermée d'elle-même — c'est sa règle de
sûreté, elle a bien joué. Mais l'écran gardait son QR : on scannait un code mort, et l'échec
ressemblait à un réseau qui ne marche pas. La fermeture s'annonce maintenant à l'interface.

**🔧 Modifications**
- ➕ `EVENEMENT_PORTE` (`sync://porte`) ; `EtatServeur.fermeture`, gardée dans la session
- ✏️ `fermer()` prévient l'interface après avoir lâché le verrou
- ➕ `ipc.onDoorClosed` ; `Recevoir` reprend son bouton et explique la fermeture subie
- ✏️ Le script de test s'arrête au premier refus au lieu de condamner la porte
- ➕ ADR-095 — un défaut qu'on ne voit pas est un défaut qu'on ne corrige pas

**🎯 Objectif**
Prouver la synchronisation continue sur les deux appareils, et ne plus jamais afficher un code
que personne n'écoute.
