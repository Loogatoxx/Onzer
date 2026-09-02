# Architecture — Onzer

> Document vivant. Toute décision structurante est consignée ici sous forme d'ADR
> (Architecture Decision Record) : contexte → décision → conséquences.

---

## Vue d'ensemble

```
┌──────────────────────────────────────────────────────┐
│  FRONTEND — React 19 / TypeScript / Tailwind         │
│  Rendu, animations, état d'interface                 │
└───────────────────────┬──────────────────────────────┘
                        │  IPC typée (types générés depuis Rust)
┌───────────────────────▼──────────────────────────────┐
│  commands/  — frontière IPC (aucune logique métier)   │
├──────────────────────────────────────────────────────┤
│  CŒUR MÉTIER RUST (sans dépendance Tauri)            │
│  audio · library · analysis · reco · stats · ingest   │
├──────────────────────────────────────────────────────┤
│  db/  — SQLite (WAL + FTS5)                           │
└──────────────────────────────────────────────────────┘
```

---

## ADR-001 — Tauri 2 + Rust plutôt qu'Electron

**Contexte.** L'application doit analyser le signal audio de milliers de fichiers
(extraction de features pour la recommandation) et scanner de grandes bibliothèques.
Ces tâches sont CPU-bound et inadaptées à JavaScript.

**Décision.** Tauri 2 avec un cœur métier en Rust.

**Conséquences.**
- ✅ Analyse audio et scan de bibliothèque à vitesse native.
- ✅ Empreinte mémoire faible (~80 Mo contre ~400 Mo pour un équivalent Electron).
- ✅ Liberté totale sur l'UI grâce au frontend web.
- ⚠️ Deux langages à maintenir. Mitigé par la génération automatique des types TypeScript
  depuis Rust (`specta`) : le contrat d'interface ne peut pas se désynchroniser.

---

## ADR-002 — Cœur métier isolé de l'UI (et non « cross-platform »)

> **Révisé.** La première version de cet ADR était intitulée « macOS d'abord, mais cœur
> portable Android », ce qui suggérait à tort que des compromis étaient consentis pour Android.
> Aucun ne l'était : les règles ci-dessous sont justifiées par leur seul bénéfice sur macOS.

**Contexte.** Onzer est une application **macOS uniquement**. Un éventuel portage Android
serait une **application distincte**, avec sa propre interface native — et non une version
cross-platform de celle-ci. Partager une UI entre pointeur et tactile produit une expérience
médiocre des deux côtés.

**Décision.** L'application est découpée en deux couches :

| Couche | Contenu | Réutilisable ailleurs |
|---|---|---|
| **Cœur métier** (Rust) | audio, base de données, analyse, recommandation, statistiques | Oui |
| **Couche UI** (React + Tauri) | rendu, animations, intégrations macOS | Non, et c'est voulu |

La couche UI a le **droit explicite** d'utiliser des API propres à macOS : Now Playing dans
le Centre de contrôle, touches média du clavier, intégration Raccourcis. Aucun compromis
n'est fait de ce côté.

Le cœur métier, lui, reste indépendant de la plateforme — non par ambition de portabilité,
mais parce que chacune des règles suivantes se justifie seule sur macOS :

**Conséquences — règles contraignantes :**

| Domaine | Règle | Justification **sur macOS** |
|---|---|---|
| Décodage audio | `symphonia` (pur Rust), jamais AVFoundation | Donne le contrôle à l'échantillon près, **seul moyen d'obtenir un vrai gapless et un crossfade propres**. AVFoundation ne les expose pas correctement. |
| Sortie audio | `cpal` derrière un trait `AudioOutput` | Permet de tester le moteur sans carte son et d'analyser en batch en mode silencieux. |
| Chemins de fichiers | Toujours via un `PathResolver` centralisé | **La bibliothèque vit sur un SSD externe amovible** : son point de montage change. Voir ADR-006. |
| Base de données | SQLite | Fiable, embarquée, sans serveur. |
| Cœur métier | N'importe jamais le crate `tauri` | Testable par `cargo test` sans lancer l'application. |

**⚠️ Point de vigilance : l'extraction de features audio.**
La librairie `bliss-audio` (référence pour la similarité musicale en Rust) dépend de **FFmpeg**,
lourd à empaqueter. Le moteur de recommandation ne doit donc jamais en dépendre directement :

```rust
/// Le moteur de recommandation ne connaît QUE ce trait.
/// Il ignore totalement comment le vecteur de features est produit.
trait FeatureExtractor {
    fn extract(&self, path: &Path) -> Result<FeatureVector>;
}
```

- `BlissExtractor` — implémentation de référence, disponible immédiatement.
- `NativeExtractor` — implémentation 100 % Rust (`symphonia` + `rustfft`), si l'on veut se
  débarrasser de la dépendance FFmpeg.

Changer d'extracteur ne touchera **aucune ligne** du moteur de recommandation.

---

## ADR-003 — Découpage vertical du frontend (feature-sliced)

**Contexte.** Un découpage horizontal classique (`components/`, `services/`, `pages/`) disperse
une même fonctionnalité dans tout le projet et rend chaque évolution transverse.

**Décision.** Découpage par domaine métier dans `src/features/` : `player`, `library`,
`playlists`, `discover`, `stats`, `import`. Chaque slice contient ses composants, ses hooks
et sa logique.

**Conséquences.**
- ✅ Ajouter ou supprimer une fonctionnalité reste local à un dossier.
- ✅ Le périmètre d'une modification est lisible d'un coup d'œil.
- ⚠️ Règle à tenir : une slice n'importe **jamais** l'intérieur d'une autre slice.
  Tout partage passe par `components/ui`, `hooks/`, `lib/` ou `stores/`.

---

## ADR-004 — `commands/` est une coquille sans logique

**Contexte.** La tentation classique en Tauri est d'écrire la logique métier directement
dans les fonctions `#[tauri::command]`, ce qui la rend intestable et non réutilisable.

**Décision.** Une commande valide ses entrées, appelle un module métier, mappe l'erreur,
et retourne. Rien d'autre.

**Conséquences.**
- ✅ Le cœur est testable par `cargo test` sans lancer d'application.
- ✅ Prérequis direct de l'ADR-002 (portabilité Android).

---

## ADR-005 — Les données utilisateur ne sont jamais versionnées

**Contexte.** La bibliothèque musicale, la base SQLite et les caches de pochettes vivent
à côté du code pendant le développement.

**Décision.** `.gitignore` exclut explicitement `/data/`, `/Library/`, `/Inbox/`, `*.db`
et les caches. Aucun fichier audio ni aucune donnée d'écoute ne rejoint le dépôt.

---

---

## ADR-006 — Audio sur le SSD externe, données sur le disque interne

**Contexte.** La bibliothèque musicale vit sur un SSD externe **Lexar de 954 Go, formaté en
exFAT**, amovible. Ce système de fichiers pose quatre problèmes documentés :

| Problème exFAT | Conséquence |
|---|---|
| Absence de journalisation | Une éjection sale peut corrompre le volume |
| Insensibilité à la casse | `ASAP Rocky` et `asap rocky` désignent le même dossier |
| Caractères interdits `" * / : < > ? \|` | Les noms d'artistes et d'albums doivent être assainis |
| Verrous POSIX peu fiables | SQLite y est exposé à la corruption |

S'ajoute le fait que le point de montage n'est pas stable : si un autre volume nommé `Lexar`
est déjà monté, macOS montera celui-ci sur `/Volumes/Lexar 1`.

**Décision.** Séparer strictement le contenu audio des données applicatives.

```
/Volumes/Lexar/<racine bibliothèque>/     ← fichiers audio uniquement (surtout lus)

~/Library/Application Support/Onzer/      ← disque interne, APFS journalisé
    ├── onzer.db                          ← base SQLite
    ├── artwork/                          ← cache de pochettes
    └── analysis/                         ← vecteurs de features audio
```

**Conséquences.**
- ✅ Base rapide, journalisée, jamais exposée à une corruption exFAT.
- ✅ L'application reste utilisable SSD débranché : bibliothèque, playlists et statistiques
  consultables, morceaux marqués **indisponibles** au lieu d'être supprimés.
- ✅ Les statistiques d'écoute sont sauvegardées par Time Machine sans effort.
- ✅ Le SSD ne contient que de l'audio standard : il reste lisible par n'importe quel autre
  lecteur, sans verrouillage propriétaire.

**Conséquence découverte à la compilation — les fichiers AppleDouble.**
macOS matérialise les attributs étendus dans des fichiers frères `._<nom>` sur les
systèmes de fichiers non natifs. Le dépôt lui-même vivant sur ce volume exFAT, deux
outils s'y sont cassé les dents :

| Outil | Symptôme | Parade |
|---|---|---|
| Build script de `tauri` | Parcourt les `.toml` de `target/` et panique sur `._default.toml`, qui n'est pas de l'UTF-8 | `.cargo/config.toml` déplace `target/` vers `~/Library/Caches/onzer/` — corrige le bug **et** accélère les compilations |
| `sqlx::migrate!` | Sélectionne les fichiers par extension `.sql` : ramasserait `._0001_initial.sql` comme une migration | `build.rs` purge les `._*` de `migrations/` avant l'expansion de la macro |

Ces deux parades sont la raison pour laquelle `.gitignore` exclut `._*` et pour laquelle
`build.rs` contient du code qui, hors de ce contexte, paraîtrait injustifié.

**Règles induites sur le schéma de base :**
1. Aucun chemin absolu stocké. La racine de bibliothèque vit dans les réglages ; chaque
   morceau ne stocke qu'un **chemin relatif**.
2. Chaque morceau porte un **hash de contenu**, qui permet de le ré-identifier même s'il a
   été déplacé ou renommé manuellement.
3. Un morceau introuvable est marqué `is_available = 0`, jamais supprimé.

---

## ADR-007 — Convention de rangement des fichiers

**Décision.** Onzer **déplace** les fichiers importés et les range selon :

```
<racine>/<Artiste>/<Année> - <Album>/<NN> - <Titre>.<ext>
```

**Cas particuliers, traités explicitement :**

| Cas | Destination |
|---|---|
| Aucun album (single) | `<Artiste>/Singles/<Titre>.<ext>` |
| Année inconnue | `<Artiste>/<Album>/…` (sans préfixe) |
| Featuring | Rangé sous l'**album artist**, jamais sous « X feat. Y » |
| Artiste non identifiable | `_À trier/` — l'utilisateur garde la main |

**Assainissement obligatoire des noms** (contrainte exFAT) : suppression des caractères
`" * / : < > ? \ |`, des points et espaces en fin de segment, troncature à 255 octets, et
résolution des collisions dues à l'insensibilité à la casse.

**Conséquence — réversibilité.** L'import déplace des fichiers, ce qui est une opération
destructive du point de vue de l'utilisateur. Chaque déplacement est donc journalisé dans
`import_jobs` avec le chemin d'origine, afin de rester annulable.

---

## ADR-008 — Journal d'écoute immuable, agrégats dérivés

**Contexte.** La recommandation et les statistiques se nourrissent des mêmes données.
Le réflexe habituel — incrémenter un compteur `play_count` sur chaque morceau — écrase
l'information au fil de l'eau : la question « qu'est-ce que j'écoutais le mardi soir en
novembre ? » devient définitivement sans réponse, et changer la formule de scoring de la
recommandation obligerait à repartir de zéro.

**Décision.** `play_events` est un **journal append-only**, protégé par un trigger.
Tous les compteurs, scores d'affinité et profils de contexte en sont **dérivés** et
donc entièrement recalculables.

Détail complet et justification de chaque colonne : [`DB_SCHEMA.md`](DB_SCHEMA.md).
Le SQL commenté fait foi dans `src-tauri/migrations/`.

**Conséquences.**
- ✅ Toute évolution d'algorithme est rejouable sur l'historique complet.
- ✅ Six signaux comportementaux capturés dès la v1 — dont `skip_at_ms` (position exacte
  du skip) et `source` (origine de l'écoute), sans lesquels la qualité du moteur serait
  invérifiable.
- ⚠️ Coût : environ 10 Mo par an pour 5 000 morceaux. Négligeable, donc **aucune purge
  n'est prévue** : l'historique est conservé à vie.
- ⚠️ Règle à tenir : un morceau n'est **jamais** supprimé physiquement. `is_available = 0`
  signale une absence temporaire (SSD débranché), `deleted_at` un retrait volontaire.
  Dans les deux cas, l'historique survit.

**Vérification.** Quatre garde-fous sont couverts par des tests dans
`src-tauri/src/db/mod.rs` : trigger append-only, clés étrangères actives, disponibilité
de FTS5 et insensibilité aux accents.

---

## ADR-009 — Politique de stockage : le volumineux va sur le SSD

**Contexte.** Le disque interne du Mac dispose d'environ 65 Go libres sur 228 Go.
L'audit a montré que le poste le plus lourd n'était pas les données de l'application
mais **le cache de compilation Rust : 3,1 Go dès les premiers builds**, contre 284 Ko
pour la base. L'ADR-006 le plaçait sur le disque interne pour contourner exFAT ;
c'était le mauvais arbitrage.

**Décision.** Règle générale : **tout ce qui est volumineux ou régénérable va sur le
SSD ; seul ce qui est petit et doit survivre au débranchement reste en interne.**

| Donnée | Emplacement | Volume estimé | Justification |
|---|---|---|---|
| Fichiers audio | SSD Lexar | Des dizaines de Go | Le contenu, par nature |
| Cache de compilation | SSD, image APFS | 3 à 8 Go | Volumineux et entièrement régénérable |
| Base SQLite | Disque interne | ~10 Mo/an | Doit rester lisible SSD débranché, et exFAT n'est pas journalisé |
| Vignettes de pochettes | Disque interne | ~30 Mo | L'interface doit rester illustrée SSD débranché |
| Pochettes pleine résolution | *Non stockées* | 0 | Déjà présentes dans les fichiers audio ; relues à la demande |
| Vecteurs d'analyse audio | Base SQLite | < 1 Mo | 20 flottants par morceau : négligeable |

Empreinte totale sur le disque interne après plusieurs années d'usage : **moins de
100 Mo**.

**Le cas du cache de compilation.** Il ne pouvait aller ni sur le disque interne (trop
volumineux) ni directement sur le volume exFAT (fichiers AppleDouble faisant planter le
build script de Tauri). La sortie par le haut est une **image disque APFS posée sur le
SSD** : stockage physique sur le Lexar, système de fichiers natif.

```
/Volumes/Lexar/Perso/Projet/.onzer-build-cache.sparsebundle
        └── monté sur /Volumes/OnzerBuild  (APFS, 60 Go, « sparse »)
```

L'image est *sparse* : elle n'occupe que l'espace réellement utilisé (33 Mo à vide).
`npm run app` monte le volume automatiquement via
[`tools/build-cache/ensure-mounted.sh`](../tools/build-cache/ensure-mounted.sh).

**Conséquences.**
- ✅ 3,1 Go rendus au disque interne, et les builds sont plus rapides qu'en exFAT
  (APFS gère bien mieux les dizaines de milliers de petits fichiers d'un build Rust).
- ⚠️ Le volume doit être monté pour compiler. Sans nouveau risque : le projet lui-même
  vivant sur le Lexar, un SSD débranché empêchait déjà toute compilation.
- ⚠️ Règle à tenir pour la suite : **toute nouvelle donnée volumineuse va sur le SSD.**
  Le disque interne est réservé à ce qui doit survivre au débranchement.

---

## ADR-010 — Import : dédoublonnage en deux passes, déplacement réversible

**Contexte.** L'utilisateur ne veut « pas gérer les métadonnées chiantes à la main ».
L'import doit donc absorber des fichiers de qualité très inégale — tags complets, tags
partiels, ou aucun tag — et éviter les doublons sans intervention.

**Décision — l'ordre des opérations n'est pas arbitraire :**

| # | Étape | Pourquoi à cette place |
|---|---|---|
| 1 | Empreinte de contenu | Bien moins coûteux que de décoder les tags. Élimine les doublons stricts d'emblée |
| 2 | Lecture des métadonnées | Repli sur le nom de fichier si les tags manquent |
| 3 | Dédoublonnage par tags | Rattrape le même titre ré-encodé, invisible à l'empreinte |
| 4 | Calcul du chemin + résolution de collision | |
| 5 | **Déplacement du fichier** | En dernier : un doublon ne doit jamais faire bouger un fichier |
| 6 | Vignette de pochette | Un échec ici n'échoue pas l'import : un morceau sans pochette reste écoutable |
| 7 | Insertion en base | Transaction unique : pas d'artiste orphelin en cas d'échec |

**L'artiste fait partie du critère de dédoublonnage.** S'en tenir au titre et à la durée
produit des faux positifs sur les titres génériques : deux albums différents possèdent
très souvent chacun une piste « Intro » de durée voisine. *Ce défaut a été trouvé par un
test, pas par relecture.*

**Découpage des artistes : volontairement conservateur.** Seules les mentions explicites
de featuring (`feat.`, `ft.`, `featuring`…) sont découpées. Découper sur `&` ou sur la
virgule démantèlerait « Earth, Wind & Fire » ou « Simon & Garfunkel ». Mieux vaut un
artiste composé qu'un faux découpage.

**Conséquences.**
- ✅ Un fichier corrompu au milieu de 3 000 titres n'interrompt pas le scan : les échecs
  sont comptés et rapportés.
- ✅ Chaque déplacement est journalisé dans `import_jobs` avec son chemin d'origine :
  l'opération reste annulable.
- ⚠️ Le dédoublonnage par tags reste une heuristique. Une empreinte du **signal audio**
  serait plus fiable, mais coûte 1 à 2 s de CPU par fichier ; elle sera calculée
  gratuitement lors de l'analyse pour la recommandation, et pourra alors affiner ce
  premier tri.

---

## ADR-011 — Moteur audio : `rodio` d'abord, pipeline maison si nécessaire

**Contexte.** L'ADR-002 imposait `symphonia` pour le décodage et `cpal` pour la sortie.
Deux façons de les assembler :

| Approche | Coût | Bénéfice |
|---|---|---|
| Pipeline maison `cpal` + `symphonia` | ~800 lignes de code temps réel : tampon circulaire, rééchantillonnage, thread sans allocation | Contrôle total : gapless à l'échantillon près, crossfade |
| `rodio` (qui encapsule exactement `symphonia` + `cpal`) | Une dépendance | Décodage, rééchantillonnage, volume et déplacement déjà résolus et éprouvés |

**Décision.** `rodio` pour la version 1. Ce n'est pas un renoncement à l'ADR-002 : `rodio`
**est** `symphonia` + `cpal`, avec la plomberie déjà faite. Écrire un pipeline temps réel
maison dès la première version reviendrait à prendre le risque le plus élevé du projet
avant même de savoir si l'application sert.

**Conséquences.**
- ✅ Lecture fonctionnelle et testée de bout en bout dès maintenant.
- ✅ Rééchantillonnage et gestion du volume gratuits.
- ⚠️ **Le gapless strict et le crossfade ne sont pas là.** L'enchaînement passe par la boucle
  de surveillance, ce qui laisse un intervalle de l'ordre de 250 ms entre deux morceaux.
  Acceptable sur des albums ordinaires, audible sur un album à enchaînements continus.
- ⚠️ La position rapportée par `rodio` suit l'écoulement du tampon de sortie et non
  l'intention de l'utilisateur : elle est donc **figée explicitement pendant les pauses**,
  faute de quoi l'horloge de l'interface tressaute. *Défaut trouvé par un test.*

**Porte de sortie.** Le reste du moteur ne connaît `rodio` que par `audio/device.rs`.
Remplacer cette seule couche par un pipeline `cpal` + `symphonia` maison ne toucherait ni la
file, ni le journal d'écoute, ni les commandes.

---

## ADR-012 — Le journal d'écoute est une machine à états à temps injecté

**Contexte.** Chaque écoute doit produire une ligne de `play_events` fidèle, y compris dans
les cas tordus : pause d'une heure, retour en arrière pour réécouter un passage, saut à trois
secondes, changement de morceau en cours de route.

**Décision.** `audio/tracking.rs` est une machine à états **sans aucune dépendance audio**,
à qui le temps est toujours **injecté** plutôt que lu depuis l'horloge.

**Conséquences.**
- ✅ Une écoute de trois minutes se teste en une microseconde, et des scénarios impossibles
  à reproduire à la main deviennent triviaux.
- ✅ Trois durées sont distinguées là où la plupart des lecteurs n'en gardent qu'une :

  | Durée | Définition | Ce qu'elle mesure |
  |---|---|---|
  | Temps écoulé | `fin − début` | Rien : inclurait une pause de trois heures |
  | **Temps écouté** | Somme des périodes de lecture réelle | L'intérêt porté au morceau |
  | **Position** | Où l'on se trouve dans le morceau | Le point de rejet, en cas de saut |

  Réécouter un passage rend le *temps écouté* supérieur à la *position finale* — un signal
  d'appréciation fort, que seule cette distinction permet de capter.
- ✅ Passer au suivant dans les deux dernières secondes est requalifié en écoute complète :
  compter cela comme un rejet fausserait durablement le score du morceau.

---

## ADR-013 — Import automatique : dossier surveillé + API locale

**Contexte.** L'utilisateur veut brancher un script de téléchargement sans « gérer les
métadonnées chiantes à la main ». Le script doit rester totalement indépendant d'Onzer.

**Décision.** Deux voies d'entrée vers le **même** pipeline d'import. Contrat complet :
[`IMPORT_API.md`](IMPORT_API.md).

### Le dossier surveillé est scruté, pas notifié

`notify` paraît plus élégant qu'un scrutin, mais échoue sur ce cas d'usage précis :

| Situation | `notify` | Scrutin toutes les 2 s |
|---|---|---|
| Fichier déposé application fermée | ❌ Événement perdu à jamais | ✅ Vu au démarrage suivant |
| **Téléchargement en cours** | ❌ Événement dès la création, fichier incomplet | ✅ Attend la stabilisation |
| exFAT | Support inégal | ✅ Indifférent |

Le second point est décisif : `yt-dlp` crée le fichier **avant** de le remplir. Importer
sur l'événement de création reviendrait à avaler un MP3 tronqué, à le taguer de travers et
à le ranger définitivement. Un fichier n'est donc importé qu'après **3 secondes de taille
inchangée**.

### Un jeton, même sur la boucle locale

Écouter sur `127.0.0.1` n'isole de rien : toute application de la machine peut y émettre des
requêtes, **y compris une page web ouverte dans un navigateur**. Sans jeton, un site visité
pourrait lire la bibliothèque ou y injecter des fichiers. Le jeton est généré au premier
démarrage, stocké en `chmod 600`, et comparé en **temps constant**.

### `GET /exists` — la route qui compte

Elle permet au script de demander « ai-je déjà ce morceau ? » **avant** de télécharger.
Économiser un téléchargement vaut mieux que le rejeter comme doublon après coup.

**Conséquences.**
- ✅ Un script n'a besoin que de trois requêtes HTTP, ou de rien du tout s'il utilise le
  dossier de dépôt.
- ✅ Arbitrage des métadonnées explicite : un vrai tag bat une indication, une indication bat
  une déduction faite depuis le nom de fichier.
- ⚠️ Un import échoué laisse le fichier dans le dépôt et **n'est pas retenté** — sans quoi un
  fichier corrompu serait réessayé toutes les six secondes indéfiniment. *Défaut trouvé par
  un test.*
- ⚠️ Le dépôt est exclu du scan de bibliothèque, sinon un téléchargement en cours y serait
  indexé.

---

## ADR-014 — « Importer un dossier » range toujours

**Contexte.** La commande d'import passait en mode « indexer sans déplacer » lorsque le
dossier choisi se trouvait dans la bibliothèque. L'intention était de gérer le rescan d'une
bibliothèque déjà organisée.

**Le défaut.** Deux intentions très différentes étaient confondues :

| Ce que fait l'utilisateur | Ce qu'il attend | Ce qui se passait |
|---|---|---|
| Rescanner sa bibliothèque rangée | Ne rien déplacer | ✅ Correct |
| Déposer des fichiers dans le dossier puis cliquer « Importer » | **Qu'ils soient rangés** | ❌ Laissés en vrac à la racine |

*Constaté sur la bibliothèque réelle de l'utilisateur : un morceau importé était resté à la
racine au lieu d'être classé sous `Damso/Singles/`.*

**Décision.** L'import range **toujours**. Un fichier déjà exactement à sa place n'est
simplement pas déplacé : `resolve_collision` exclut désormais le fichier source de la
détection de collision, faute de quoi réimporter une bibliothèque rangée renommerait chaque
morceau en « … (2) ».

Le mode `IndexInPlace` reste disponible pour une future commande de rescan explicite.

---

## ADR-015 — Un seul morceau analysé à la fois

**Contexte.** L'analyse d'un morceau exécute environ 3 800 transformées de Fourier et
sature un cœur pendant toute sa durée. Sur cinq mille morceaux, une exécution naïve fige
l'interface et fait hoqueter la lecture.

**Décision.** L'ouvrier traite **un seul morceau à la fois**, sur un fil bloquant dédié,
avec 250 ms de repos entre deux.

Paralléliser sur quatre cœurs diviserait le temps total par quatre, mais rendrait
l'application désagréable pendant toute la durée du traitement. *Une analyse invisible qui
prend une heure vaut mieux qu'une analyse rapide qui rend l'application inutilisable.*

**Mesures réelles** (Apple Silicon, MP3 de 8 Mo) :

| | Avant correction des profils | Après |
|---|---|---|
| Par morceau | 6 591 ms | **267 ms** |
| 5 000 morceaux | ~9 heures | **~45 minutes** |

Le facteur 24,7 vient de deux lignes de `Cargo.toml` : les dépendances DSP étaient
compilées **sans optimisation en développement**, et le profil de publication visait la
taille du binaire (`opt-level = "s"`) plutôt que la vitesse. *Défaut trouvé en mesurant,
pas en relisant.*

**Conséquences.**
- ✅ Duty cycle d'environ 50 % d'un seul cœur : imperceptible sur une machine multicœur.
- ✅ Reprise après redémarrage : un morceau resté « running » est remis en file.
- ✅ Les morceaux récemment ajoutés passent en premier — ce sont ceux qu'on veut écouter.
- ⚠️ La durée d'analyse est journalisée en permanence, pour détecter toute régression.

---

## ADR-016 — Le quota d'artistes s'adapte à la bibliothèque

**Contexte.** Les règles de diversité plafonnaient à trois morceaux par artiste et par
playlist.

**Le défaut.** Ce plafond rend une playlist de vingt titres **mathématiquement impossible**
dans une bibliothèque de six artistes : 6 × 3 = 18. Le moteur produisait alors des
playlists mystérieusement courtes, sans que rien ne l'explique. *Défaut trouvé par un test
d'assemblage, invisible sur les modules pris isolément.*

**Décision.** Le quota est calculé pour que la longueur demandée reste atteignable, avec
trois emplacements de marge — la sélection étant gloutonne, elle a besoin de jeu. Le délai
de carence subit la même contrainte : avec deux artistes, exiger deux morceaux d'écart est
irréalisable.

**Conséquence.** Sur une grande bibliothèque, le quota reste à trois : la règle ne se
relâche que là où elle serait contradictoire.

---

## ADR-017 — Charte visuelle : monochrome, Avenir Next, icônes dessinées à la main

**Contexte.** L'interface avait été montée « au plus simple » en attendant le travail
graphique. Elle accumulait des dégradés violet → cyan sur de grandes surfaces, quatre
couleurs néon dans les statistiques, et des icônes redessinées à chaque usage avec des
épaisseurs de trait différentes.

**Décision — la couleur.** Toute la charpente est en niveaux de gris. **Les seules couleurs
de l'écran viennent des pochettes.** Une interface musicale qui apporte ses propres couleurs
entre en concurrence avec les albums qu'elle affiche. L'accent violet ne subsiste qu'à
quatre endroits : le morceau en cours, un favori, le chiffre pivot d'une section de
statistiques, la barre de progression au survol.

**Décision — la typographie.** `Avenir Next`, livrée avec macOS. Circular, la fonte de
Spotify, est une linéale géométrique ; Avenir (Frutiger, 1988) est l'ancêtre direct de
cette famille. Elle monte jusqu'au poids Heavy, ce qui donne de vrais titres d'affichage,
et ne transite par aucun réseau. Le choix exclut délibérément les fontes « par défaut » des
interfaces générées (Inter en tête), reconnaissables au premier coup d'œil.

**Décision — les icônes.** Un fichier unique, `src/components/Icon.tsx`, plutôt qu'une
bibliothèque. Une bibliothèque apporte deux mille symboles pour en utiliser trente et impose
son style ; surtout, elle ne garantit pas ce qui fait qu'un jeu d'icônes paraît
professionnel : **un trait rigoureusement identique partout**. Grille de 24, trait de 1,75,
extrémités arrondies. Seules trois formes sont pleines — lecture, pause, cœur — parce qu'un
symbole d'action doit peser plus lourd qu'un symbole de navigation.

**Décision — les fonds dégradés.** Aucun disque flouté (`blur`). Le flou étale un contour
sans le supprimer : sur un fond très sombre, l'œil finit toujours par voir le bord du
disque — c'est précisément ce qui se voyait sur la page de statistiques. Les fonds sont
désormais des **dégradés radiaux**, qui atteignent réellement la transparence.

**Conséquence.** Les jetons néon (`neon-lime`, `neon-pink`, `neon-amber`) ont disparu des
jetons de design, et avec eux la tentation de les réutiliser.

---

## ADR-018 — Une coquille unique plutôt que des pages

**Contexte.** La bibliothèque et les statistiques cohabitaient dans une même vue via une
bascule. L'arrivée des playlists, des favoris, des paroles et de la file d'attente rendait
ce montage intenable.

**Décision.** `AppShell` compose quatre zones permanentes — barre latérale, zone principale
défilante, panneau « en cours de lecture », barre de lecture — et n'en démonte aucune lors
d'une navigation.

**Pourquoi c'est structurant.** Changer de page ne doit jamais donner l'impression d'avoir
coupé la musique. C'est aussi la seule disposition dans laquelle les paroles peuvent
continuer à défiler pendant qu'on fouille sa bibliothèque : en faire une page plein écran
obligerait à quitter ce qu'on regardait pour lire une ligne.

**Navigation.** Une pile avec curseur, et non un état courant unique : les flèches
précédent/suivant n'ont de sens que s'il existe un historique. La recherche, elle, n'entre
pas dans la pile — c'est une surimpression que l'on quitte en vidant le champ, pas une
destination.

**Commandes de lecture centrées.** Elles étaient à gauche. Au centre, elles libèrent toute
la largeur pour la barre de progression : sur un morceau de quatre minutes, c'est la
différence entre viser une seconde et en viser dix.

---

## ADR-019 — Les paroles vivent dans le fichier

**Contexte.** Chaque morceau importé peut porter des paroles dans ses tags, parfois
synchronisées au format LRC.

**Décision.** Les paroles sont lues depuis le tag `Lyrics`, analysées par
`library::lyrics`, et **réécrites dans le fichier** lorsque l'utilisateur les modifie. La
base ne sert que de cache de lecture.

**Le défaut que cette décision a révélé.** `identify::tagger::write_tags` reconstruit un
bloc de tags neuf avant de l'écrire. Omettre le champ `Lyrics` aurait **effacé les paroles
synchronisées que l'utilisateur possédait** au moment de la ré-identification acoustique —
une perte silencieuse et irréversible.

**Suivi de la lecture.** `Lyrics::line_at` fait une recherche dichotomique ; la même
logique est dupliquée côté TypeScript. Duplication assumée : l'alternative serait un
aller-retour IPC **quatre fois par seconde** pour six lignes de code.

---

## ADR-020 — L'empreinte de dédoublonnage ne couvre que l'audio

**Contexte.** `content_hash` couvre le fichier entier. Or Onzer réécrit les tags après
identification acoustique : **le fichier change d'octets sans changer de musique**.

**Le défaut, observé en production.** L'empreinte stockée cessait de correspondre au fichier
d'origine. Un second exemplaire du même téléchargement n'était donc plus reconnu — et le
dédoublonnage par tags ne rattrapait pas la chute, puisque la ligne en base portait déjà les
tags corrigés (« Stromae ») quand l'entrant portait encore les siens (« Damso »). **Les deux
filets se trouaient en même temps.** Trois exemplaires du même morceau sont entrés en base,
et 37 fichiers déjà rangés tournaient en boucle dans le dossier de dépôt, réexaminés à
chaque démarrage.

**Décision.** `audio_hash` ne couvre que les octets audio : en-tête ID3v2 sauté, queues
ID3v1 et APEv2 retirées, blocs de métadonnées FLAC parcourus, atome `mdat` pour MP4, bloc
`data` pour WAV. Retaguer déplace les bornes, jamais le contenu entre elles. Le format OGG
retombe sur le fichier entier — repli documenté, non silencieux.

**Conséquences.**

- Un doublon détecté dans le dépôt est **déplacé** dans `_Inbox/_Doublons` plutôt que laissé
  sur place : Onzer ne détruit rien, mais un fichier qui reste candidat est réexaminé
  indéfiniment.
- `library::repair` rattrape les empreintes manquantes au démarrage et fusionne ce qui doit
  l'être. L'import automatique **attend** que cette passe soit terminée : dédoublonner avant
  que les empreintes existent reviendrait à travailler à l'aveugle.
- Un doublon déjà écouté est marqué supprimé, jamais détruit : `play_events` le référence en
  `ON DELETE RESTRICT`, et perdre un historique pour faire le ménage serait pire que le mal.

---

## ADR-021 — Une empreinte acoustique est une présomption, pas une preuve

**Contexte.** Onzer acceptait toute correspondance dépassant son seuil de confiance, sans
jamais la confronter aux tags que le fichier portait déjà.

**Les deux défauts, tous deux observés sur la bibliothèque réelle.**

1. Un morceau de Damso s'est retrouvé étiqueté « carmen (Clip Officiel) » de Stromae. La
   fiche MusicBrainz retenue portait littéralement ce nom et **n'avait aucune parution** :
   une fiche versée depuis un rip YouTube. Le fichier a été retagué, déplacé dans
   `Stromae/`, et ses tags d'origine perdus.
2. « Macarena » de Damso paraît sur *Ipséité* en 2017, mais son enregistrement MusicBrainz
   n'est rattaché qu'à **neuf compilations**. Le classement départageait « à qualité égale,
   la plus ancienne — c'est la parution d'origine » et retenait *I migliori anni '90*, paru
   en **2009** : huit ans avant que le morceau existe. La règle est juste entre deux albums,
   absurde entre deux compilations.

**Décisions.**

| Garde-fou | Règle |
|---|---|
| Durée | Écart de plus de 7 s avec l'enregistrement → refus. Seul critère purement objectif |
| Tags du fichier | Une contradiction franche exige une confiance ≥ 0,90 pour passer outre |
| Fiche sans parution | Combinée à une contradiction → refus. C'est le profil du rip versé à la va-vite |
| Compilations | Quand toutes les parutions en sont, il n'y a **pas d'album connu** : on ne renvoie rien et les tags du fichier sont conservés |

Une case vide vaut mieux qu'une fausse réponse. Un refus est consigné avec sa raison
(`identification_state = 'rejected'`), et non réessayé : l'empreinte est déterministe.

**Réversibilité.** Les colonnes `original_title` / `original_artist` / `original_album` sont
renseignées à l'import et **jamais réécrites**. Sans cette mémoire, une identification
erronée est irréversible. Pour la bibliothèque déjà abîmée, `repair` retrouve ces tags dans
les exemplaires écartés du dépôt — qu'Onzer n'a jamais retagués. Les copies de bibliothèque,
elles, ne savent que répéter ce qu'Onzer a écrit : elles sont traitées en premier et
n'ont pas le dernier mot.

**L'arbitrage revient à l'utilisateur.** Une identification qui contredit les tags a le plus
souvent raison — c'est sa raison d'être. Onzer montre les deux versions côte à côte et
laisse choisir.

---

## ADR-022 — Le réseau enrichit, il ne conditionne jamais

**Contexte.** Un relevé des 192 fichiers de la bibliothèque de test n'a trouvé **aucune**
frame `USLT` ni `SYLT` : pas un seul ne portait de paroles. Les afficher supposait d'aller
les chercher.

**Décision.** LRCLIB — sans clé d'API, sans compte, avec des paroles synchronisées au format
LRC. Ce qui part se limite à l'artiste, au titre, à l'album et à la durée.

**La frontière.** Onzer reste un lecteur hors ligne. Aucun appel n'est déclenché sans un
geste explicite de l'utilisateur : un bouton dans le panneau de lecture, un autre pour la
bibliothèque entière. Sans réseau, la musique s'écoute exactement pareil.

**Les paroles vivent dans le fichier.** Écrites dans le tag, pas seulement en base : une fois
trouvées, elles ne dépendent plus du réseau et suivent le morceau ailleurs. La lecture
retombe d'ailleurs sur le fichier quand la base est muette — la colonne `lyrics` n'ayant été
ajoutée qu'après coup, s'en tenir à elle revenait à répondre « pas de paroles » pour toute
une bibliothèque.

---

## ADR-023 — L'accueil propose des entrées, pas la bibliothèque

**Contexte.** Une bibliothèque personnelle a un défaut que les catalogues en ligne n'ont
pas : **on l'a déjà toute entendue**. Ouvrir sur une liste de 253 morceaux connus ne donne
envie de rien.

**Décision.** L'accueil devient la page d'ouverture et ne montre aucune liste. Trois rayons,
répondant chacun à une question différente :

| Rayon | Question |
|---|---|
| Reprendre | « Qu'est-ce que j'écoutais ? » |
| Mix du jour | « Fais-moi écouter un artiste que j'aime, sans que je choisisse » |
| Faits pour toi | « Surprends-moi, dans une direction que je choisis » |

**Trois nouveaux types de mix**, avec une règle que les précédents n'avaient pas : certains
**brident le bandit**. Il sert à découvrir ce qui marche quand la question est ouverte
(« que me proposer maintenant ? »), mais un mix nommé « Jamais écoutés » y a déjà répondu.
Le laisser piocher dans l'affinité produirait une playlist de titres archi-connus sous un
nom qui promet l'inverse — et rien ne détruit plus vite la confiance qu'une promesse
démentie au premier titre.

| Mix | Stratégies autorisées | Filtre |
|---|---|---|
| `loved` | Affinité seule | — |
| `never_played` | Découverte seule | `play_count = 0` |
| `artist_mix` | Similarité + découverte | — |

Un mix d'artiste prend pour référence le **barycentre sonore de tout son catalogue**, et non
un de ses morceaux : un artiste qui alterne les registres serait sinon réduit à celui du
titre tiré au sort.

**La rotation quotidienne.** Un mix identique chaque matin cesse d'être regardé au bout de
trois jours. L'ordre des artistes est permuté par le quantième du jour : la sélection ne
change pas — ce sont bien les plus écoutés — mais la tête de rangée, oui.

**Compléter plutôt que se rabattre.** Le premier jet ne consultait la bibliothèque que si
l'historique était *vide*. Sur une bibliothèque jeune il ne l'est pas : il compte deux
artistes. Le rayon affichait donc deux cartes au lieu de six. L'historique reste
prioritaire, les artistes les mieux fournis complètent — avec un plancher de quatre
morceaux, en deçà duquel un « mix » ne se distingue pas d'une playlist ordinaire.

**Pourquoi des mosaïques.** Un mix n'est pas un album. Une pochette unique le ferait passer
pour un disque, et l'on s'attendrait à y retrouver toujours les mêmes morceaux. Quatre
pochettes assemblées disent qu'il s'agit d'un assemblage, et montrent ce qu'il contient.

---

## ADR-024 — Corriger une règle ne défait pas ce qu'elle a écrit

**Contexte.** L'ADR-021 a corrigé `pick_release` : quand toutes les parutions d'un
enregistrement sont des compilations, il n'y a pas d'album connu. Mais « Macarena » restait
affiché avec la pochette d'*I migliori anni '90*, une compilation italienne de 2009 — parce
que la mauvaise valeur était déjà en base, dans les tags du fichier, et jusque dans son
chemin sur le disque.

**Décision — `identify::revise`.** Une passe, jouée une seule fois (jalonnée par un réglage),
qui interroge MusicBrainz **une fois par album** et non par morceau : tous les titres d'un
album partagent le même verdict, et 253 requêtes là où 60 suffisent n'useraient que la
patience du service.

| Verdict | Condition | Effet |
|---|---|---|
| `Replace` | Une parution non-compilation existe et porte un autre nom | Album et année corrigés |
| `Clear` | Aucune, **et** l'album en base figure parmi les parutions de la fiche | Album, année et pochette effacés |
| `Keep` | Tout le reste | Rien |

La seconde condition du `Clear` est essentielle : un album que l'utilisateur avait renseigné
lui-même et que MusicBrainz ignore ne doit **jamais** être effacé. On ne défait que ce qu'on
a soi-même écrit.

**Pourquoi effacer la pochette.** Une image fausse affirme quelque chose ; un carré vide
n'affirme rien. La pochette d'origine n'était pas récupérable — Onzer l'avait écrasée dans le
fichier, et aucune copie n'avait survécu.

**Le disque doit suivre la base.** Effacer l'album laissait le fichier dans un dossier
portant encore son nom : la base et le disque se contredisaient. `refile_without_album` range
le morceau sous `Artiste/Singles/` et supprime le dossier vidé — en n'y retirant que ce
qu'Onzer y avait lui-même déposé (`cover.jpg`). Tout autre fichier arrête le nettoyage.

---

## ADR-025 — Une playlist courte est un défaut, pas un compromis

**Contexte.** Un test du moteur échouait une fois sur une quinzaine, sans jamais être
reproductible.

**La cause.** L'ordre d'itération d'une `HashMap` est **volontairement randomisé à chaque
exécution** du processus Rust. Il déterminait celui des viviers de candidats, et l'addition
flottante n'étant pas associative, il changeait jusqu'au barycentre de l'espace sonore aux
derniers bits près. Deux similarités quasi ex æquo s'inversaient, et la sélection gloutonne
se retrouvait parfois coincée contre les règles de diversité.

**Ce que le hasard masquait.** Une fois l'ordre rendu déterministe, l'échec est devenu
systématique : le moteur rendait **14 titres sur 15 demandés**. Ce n'était pas un test
fragile, c'était un vrai défaut que l'aléa rendait invisible — et le même symptôme que
l'ADR-016, par une autre cause.

**Décisions.**

1. Ordre déterministe partout : viviers triés par score puis par identifiant, identifiants
   triés avant tout calcul de barycentre. La génération est désormais entièrement
   reproductible pour une graine donnée, comme sa documentation le promettait déjà.
2. **Passe de complétion** : si la sélection s'arrête avant la longueur demandée, elle
   termine avec les règles de **confort** levées — délai de carence et quota par artiste. La
   règle **dure** reste entière : jamais deux fois le même morceau. Un titre moins bien
   espacé vaut mieux qu'un titre manquant ; un doublon, lui, se verrait.

Le test correspondant vérifie la propriété sur cinq longueurs et quatre graines : un cas
unique aurait pu passer par chance.

---

## ADR-026 — Les catégories sont des quartiles de ta propre bibliothèque

**Contexte.** Sur 325 morceaux, **deux** portaient un genre dans leurs tags. Un classement
par genre afficherait deux catégories et un grand vide. L'analyse audio, elle, couvre tout.

**Le piège des seuils absolus.** Sur cette bibliothèque — du rap, essentiellement —
l'énergie médiane vaut **0,86**. Une règle « énergique = au-dessus de 0,7 » y retiendrait les
trois quarts des morceaux et ne distinguerait rien. Un seuil écrit en dur ne veut rien dire
hors du corpus qui l'a inspiré.

**Décision.** Chaque catégorie est un **quartile de la bibliothèque elle-même**. « Ça tape »
désigne les 25 % les plus énergiques *de ta musique*, quelle qu'elle soit. La règle se
déplace avec la collection et n'a jamais à être réglée.

Mesuré sur la bibliothèque de test : seuil d'énergie à 0,96 pour 82 morceaux, seuil de
valence basse à 0,39 pour 81 — des tranches équilibrées qu'aucune constante n'aurait pu
deviner.

**Ce sont des filtres, pas des playlists générées.** « Montre-moi mes morceaux calmes » est
une question à réponse exacte ; la faire passer par le moteur de recommandation y ajouterait
diversité, exploration et hasard — trois façons de ne pas répondre à la question posée.

**Sécurité du SQL.** Les noms de colonnes sont interpolés, mais viennent tous d'une table
constante du programme ; la clé fournie par l'interface est résolue par `definition()` avant
d'atteindre la moindre requête, et le seuil reste un paramètre lié. Un test vérifie qu'une
clé inventée est refusée.

---

## ADR-027 — Le seul endroit qui parle de ce que tu n'as pas

**Contexte.** Le moteur ne connaît que la bibliothèque. Il ne peut pas, par construction,
suggérer un artiste absent.

**Décision.** ListenBrainz, en données ouvertes, sans clé ni compte. La recette de similarité
retenue est bâtie sur les **sessions d'écoute** — deux artistes sont proches s'ils sont
souvent écoutés dans une même séance —, une notion plus proche de l'usage réel que du genre
déclaré.

**Ce qui sort de la machine** se limite à des identifiants MusicBrainz d'artistes : pas un
titre, pas une écoute, pas un horodatage. Et rien ne part sans un clic.

**La méthode : faire voter tes artistes.** Les voisins de chacun de tes cinq artistes les
plus fournis sont agrégés, et **leurs scores s'additionnent**. Prendre les meilleurs voisins
d'un seul donnerait sa discographie élargie ; les faire voter fait ressortir ce qui se
trouve au *centre* de tes goûts plutôt qu'à la périphérie de l'un d'eux. Ceux que tu possèdes
déjà sont écartés.

**Onzer ne télécharge rien** (voir la discussion de l'ADR sur l'identification acoustique) :
il nomme des artistes, à charge pour l'utilisateur d'aller les chercher où il en a
l'habitude.

---

## ADR-028 — Une fonction de base sans test est une erreur en attente

**Contexte.** `restore_identity` écrivait dans `tracks_fts` avec des noms de colonnes
inventés — `artists` et `album` là où le schéma dit `artist_names` et `album_title`. La
transaction échouait, et l'utilisateur lisait « no such column: artists » en cliquant sur
« Rétablir ».

**Pourquoi rien ne l'a arrêté.** Le compilateur ne vérifie pas le SQL. Aucun test ne couvrait
la fonction, et les 472 autres passaient. Le défaut n'existait que sur le chemin exact que
l'utilisateur a emprunté.

**Décision.** Toute fonction qui écrit dans `tracks_fts` doit être couverte par un test qui
**relit l'index** — c'est-à-dire qui cherche le morceau après coup. Vérifier la ligne de
`tracks` ne prouve rien : l'index est une table séparée, et c'est précisément là que la
faute se logeait.

**Conséquence de style.** L'index se réécrit par `DELETE` puis `INSERT`, comme partout
ailleurs dans `repository.rs`. Mélanger deux façons d'écrire une table FTS5 est le meilleur
moyen de se tromper de colonnes.

---

## ADR-029 — Retirer n'est pas détruire

**Contexte.** Le menu d'une ligne doit pouvoir retirer un morceau de la bibliothèque.

**Décision.** La ligne est **marquée** supprimée, jamais effacée, et le fichier sur le disque
n'est pas touché.

**Pourquoi.** `play_events` référence `tracks` en `ON DELETE RESTRICT` : effacer la ligne
emporterait l'historique d'écoute, dont vivent les statistiques et le moteur de
recommandation. Perdre des mois d'historique parce qu'on ne veut plus voir un titre dans une
liste serait un marché absurde. Quant aux octets, les supprimer supposerait de pouvoir
revenir en arrière — ce n'est pas le rôle d'un lecteur.

L'action est en **deux temps** dans le menu, isolée sous un séparateur, et accompagnée de la
phrase qui dit ce qui survit. Un geste irréversible doit annoncer sa portée avant d'être
exécuté, pas après.

**La colonne de paroles.** `TrackSummary` porte un booléen, jamais le texte : renvoyer les
paroles de trois cents lignes pour afficher une pastille représenterait des centaines de
kilo-octets à chaque affichage de liste.

**La page d'un artiste** compte ses featurings, sa fiche dans la liste ne compte que ses
titres principaux. On ouvre un artiste pour tout ce qu'il a fait ; on ne veut pas voir dans
la liste cent noms dont on ne possède qu'une apparition. Vignettes rondes, par la convention
qui distingue une personne d'un objet — un carré, c'est un disque.

---

## ADR-030 — Le décodeur doit être construit *seekable*

**Contexte.** Cliquer en arrière dans la barre de progression ne faisait rien. En avant,
cela semblait fonctionner.

**La cause, lue dans les journaux.** Chaque saut échouait — `saut impossible sur ce format :
Symphonia decoder returned an error` — dans les deux sens. L'illusion que l'avant marchait
venait de la lecture qui continuait d'avancer toute seule.

`open_decoder` appelait `rodio::Decoder::new(BufReader::new(file))`. Ce constructeur ne
déclare **ni la source comme seekable, ni sa taille en octets**. La documentation de rodio
est explicite : pour un fichier, il faut préférer `TryFrom<File>`, qui renseigne les deux.

**Décision.** `rodio::Decoder::try_from(file)`. Le décodage reste en flux depuis le disque.

**La leçon.** Un avertissement journalisé sans être remonté à l'interface est un défaut qui
attend. Celui-ci était écrit à chaque clic depuis des semaines ; il a fallu qu'un utilisateur
décrive le symptôme pour qu'on aille lire.

---

## ADR-031 — Les doublons probables se montrent, ils ne se fusionnent pas

**Contexte.** Deux entrées d'un même titre subsistent quand leur audio diffère — un clip et
sa version album, séparés de dix secondes. Le dédoublonnage automatique s'arrête à deux
secondes, à raison : au-delà, ce sont deux enregistrements différents.

**Décision.** Onzer les **rapproche** sans trancher, avec ce qu'il faut pour décider : durée,
album, chemin, et nombre d'écoutes — c'est presque toujours celui qu'on a écouté qu'on garde.
Tolérance de vingt secondes, assez pour couvrir les versions courtes sans rapprocher deux
homonymes sans rapport.

**Pourquoi ne pas fusionner d'autorité.** Les deux fichiers sont réellement différents, et
souvent tous deux légitimes. Une fusion automatique ferait disparaître une version que
l'utilisateur voulait garder, sans qu'il l'ait demandé — et il n'aurait aucun moyen de savoir
laquelle a disparu.

---

## ADR-032 — Les paroles ont deux tailles, une seule logique

**Contexte.** Le panneau latéral fait 22 rem : de quoi suivre une ligne du coin de l'œil, pas
de quoi lire.

**Décision.** Une page pleine largeur, où le texte devient l'illustration — la ligne chantée
pleinement lisible, les autres estompées. Ce contraste n'est pas un effet : c'est ce qui
permet de retrouver l'endroit d'un coup d'œil après avoir regardé ailleurs.

**Un seul `useLyrics`.** Chargement, ligne courante, recentrage automatique, recherche en
ligne, saisie manuelle : tout vit dans le hook, partagé par les deux affichages. Ce qui
change entre eux est la mise en page, jamais le comportement — dupliquer la logique
garantirait qu'une correction n'atteigne qu'un des deux.

**Les flèches du clavier** déplacent de 5 s, et le pas s'élargit au maintien (5 s, puis 15 s,
puis 30 s). Un pas fixe imposerait une trentaine d'appuis pour traverser un morceau ; le
compteur se remet à zéro au relâchement, sans quoi deux appuis distincts s'additionneraient.

---

## ADR-033 — Onzer compare, il ne télécharge pas

**Contexte.** L'utilisateur veut coller un lien de playlist Spotify et savoir ce qui manque
dans sa bibliothèque, puis automatiser la récupération avec `spotdl`, l'outil qu'il utilise
déjà.

**La ligne que ce projet tient.** Elle a été posée bien plus tôt, quand la première demande
était d'embarquer un téléchargeur : Onzer n'en embarque pas, n'en pilote pas, n'en exécute
pas. Ce qu'il fait ici est un travail de **bibliothécaire** — lire une liste de titres, la
confronter à ce qu'il possède, nommer les absents. La formule proposée pour le terminal
assemble ce que l'utilisateur taperait autrement à la main, pour son propre outil ; il la
copie, ou non.

**Décision, révisée — un fichier plutôt que l'API.** Le premier jet interrogeait l'API
officielle avec des identifiants d'application créés par l'utilisateur. Spotify a répondu :

> 403 Forbidden — *Active premium subscription required for the owner of the app.*

Lire la liste d'une playlist **publique** exige désormais un abonnement payant sur le compte
propriétaire de l'application. La porte est fermée, et aucune astuce ne la rouvrirait
honnêtement — scraper la page reviendrait à passer par la fenêtre, avec un analyseur qui
casserait au premier changement de leur front-end.

`spotdl`, que l'utilisateur a déjà installé, produit la liste avec sa sous-commande `save` :
**sans rien télécharger**, et sans aucun identifiant. Onzer lit ce fichier. Le partage des
rôles ne change pas, et c'est ce qui compte : son outil va chercher la liste, Onzer la
compare.

Le format est lu **avec indulgence** — noms de champs alternatifs, durée en secondes ou en
millisecondes, entrées non résolues ignorées. `spotdl` évolue, et une clé renommée ne doit
pas rendre tout le fichier illisible. Vérifié sur une vraie playlist : 548 titres lus,
64 déjà en bibliothèque, 484 manquants.

**Le rapprochement réutilise `find_by_tags`** — titre normalisé, artiste normalisé, durée à
deux secondes près. C'est **la même** règle que le second filet du dédoublonnage à l'import :
un morceau déclaré présent ici doit être exactement celui que l'import refuserait comme
doublon. Deux règles distinctes finiraient par se contredire sous les yeux de l'utilisateur.

**La boucle presque automatique.** Les requêtes manquantes sont écrites dans un fichier, et
la commande vise le **dossier de dépôt**. Ce qui est récupéré y atterrit ; dédoublonnage,
identification et rangement se font ensuite tout seuls. `xargs` découpe lui-même : sur la
playlist de test, 484 titres manquaient — écrite d'un bloc, la commande aurait fait une
vingtaine de kilo-octets.

---

## ADR-034 — Découvrir des titres : la discographie, pas la similarité

**Contexte.** La section « à découvrir » ne proposait que des artistes. L'utilisateur voulait
aussi des **titres** qu'il pourrait aimer et qu'il ne possède pas.

**La piste évidente, et pourquoi elle échoue.** ListenBrainz expose des enregistrements
similaires. Interrogé sur les enregistrements de cette bibliothèque, il répond
**systématiquement une liste vide** : le rap francophone y est trop peu représenté pour que
la similarité par sessions d'écoute produise quoi que ce soit. Livrer une section
structurellement vide aurait été pire que de ne rien livrer.

**Décision.** La **discographie** des artistes déjà aimés, via MusicBrainz — complète et
fiable là où la similarité est muette : 194 enregistrements connus pour Damso. On la compare
au catalogue possédé et l'on nomme la différence. Cela répond exactement à la question posée
— « des titres que je pourrais aimer et que je n'ai pas » — sans rien inventer.

**Deux précautions.** Les enregistrements homonymes d'un même artiste — live, remix,
réédition — sont réduits à un seul : sous le même nom, ils n'apprennent rien de plus. Et la
liste est **entrelacée** entre artistes : trente titres du même artiste ressembleraient à une
discographie, pas à une découverte.

**Où elle se trouve.** Le premier jet plaçait cette section tout en bas de l'accueil, sous
les mix et les catégories. L'utilisateur ne l'a jamais vue et a demandé si elle existait.
Elle vit désormais dans « Ce qui me manque », dont c'est exactement le sujet, et en tête de
page — elle fonctionne d'un clic, sans rien installer.

---

## ADR-035 — Un doublon probable s'écoute avant de se trancher

**Contexte.** Le panneau des doublons montrait durée, album et nombre d'écoutes. Cela ne
suffit pas : entre un clip et sa version album, seule l'oreille tranche.

**Décisions.**

1. Un bouton de lecture par ligne, qui joue le morceau **seul** sans toucher à la file
   affichée. Comparer suppose d'écouter l'un puis l'autre, pas de perdre sa file en route.
2. Un bouton « Ce ne sont pas des doublons » par groupe. Deux morceaux peuvent porter le
   même titre et durer presque pareil sans avoir de rapport — une reprise, deux interludes
   homonymes. Sans ce bouton, le panneau redemanderait éternellement.

La décision est stockée dans les réglages, pas dans une table : c'est une poignée de clés,
et son cycle de vie est celui d'une préférence. Elle reste donc **réversible**, ce qu'une
suppression ne serait pas.

---

## ADR-036 — Les identifiants Spotify servent à l'outil, plus à Onzer

**Contexte.** Après la bascule vers `spotdl save`, l'utilisateur a rencontré, sur une
playlist de 550 titres, un `TypeError: 'NoneType' object is not subscriptable` au fond de la
couche réseau de `spotdl`. Interrogé anonymement, Spotify éconduit les grandes requêtes.

**Décision.** Onzer conserve les identifiants d'application saisis par l'utilisateur — ils
étaient devenus inutiles quand l'API nous a été fermée — et les **glisse dans les commandes
qu'il propose** (`--client-id`, `--client-secret`). Il ne s'en sert lui-même pour aucun
appel.

Le chemin de l'exécutable est résolu, pas supposé : `spotdl` s'installe le plus souvent via
`pipx`, dans un dossier absent du `PATH` d'un shell non interactif. Une commande qui échoue
sur « command not found » alors que l'outil est installé envoie chercher un problème qui
n'existe pas. Les emplacements habituels sont testés, avec repli sur le nom nu.

**Le secret est affiché en clair**, puisque c'est ce que la ligne de commande exige. Un
avertissement l'accompagne : cette ligne ne se colle nulle part ailleurs que dans un
terminal.

---

## ADR-037 — Deux fois le même texte n'apprend rien

**Contexte.** Les paroles en pleine page laissaient le panneau latéral afficher… les mêmes
paroles, en petit.

**Décision.** Quand la page des paroles est ouverte, l'onglet « Paroles » du panneau
disparaît et celui-ci bascule sur **« À suivre »**. La colonne de droite est la seule qui
pouvait montrer autre chose ; la remplir d'un doublon la gaspille, et laisse croire à un
défaut d'affichage.

« À suivre » plutôt qu'autre chose parce que c'est la question qui vient naturellement quand
on lit en écoutant : quel morceau ensuite.

**L'icône des artistes** suit la même logique de cohérence : une tête et des épaules sur la
grille de 24, au trait de 1,75 comme tout le reste. Un rond plein aurait juré — toutes les
formes de navigation sont en filaire, et l'œil repère immédiatement l'intruse.

---

## ADR-038 — Comparer ne doit dépendre d'aucun service

**Contexte.** Les identifiants Spotify ont été glissés dans la commande `spotdl save`, comme
demandé. Quatre mesures ont suivi :

| Voie | Résultat |
|---|---|
| API officielle + identifiants (`--use-official-api`) | **403** — abonnement payant exigé sur le compte propriétaire |
| Contournement anonyme, playlist | `BaseClientError: Could not get general hashes` |
| Contournement anonyme, liens de titres | Fichier vide, 0 titre |
| La même commande, deux heures plus tôt | **548 titres** |

Les identifiants sont donc **inertes** : sans `--use-official-api`, `spotdl` les ignore et
passe par un analyseur des pages web de Spotify ; avec, il retombe sur le 403. Les afficher
exposait le *client secret* à l'écran pour aucun bénéfice — ils ont été retirés des commandes
proposées.

**Le vrai problème.** Aller chercher la liste est le maillon fragile, et il ne nous appartient
pas : il dépend d'un scraper tiers face à une API hostile, qui marchait le matin et plus
l'après-midi. Aucune correction de notre côté ne peut le rendre fiable.

**Décision.** La comparaison — qui, elle, est notre travail — n'en dépend plus. Onzer accepte
**toute liste de titres**, reconnue automatiquement :

| Forme | D'où elle vient |
|---|---|
| JSON `.spotdl` | `spotdl save`, quand il fonctionne |
| CSV | Exportify et la plupart des exportateurs |
| Texte brut | Un copier-coller, une liste écrite à la main |

Deux détails d'analyse méritent d'être notés, parce qu'ils cassent en silence sinon : le
découpage CSV **respecte les guillemets** (« Penelope, pt. 2 » contient une virgule), et seul
un tiret **entouré d'espaces** sépare l'artiste du titre (« Jay-Z » ne doit pas être coupé).

Vérifié sur la playlist réelle de 548 titres, convertie en texte : 548 analysés, artistes et
titres corrects.

**La voie `spotdl` reste proposée**, repliée derrière un dépliant et accompagnée de ce
qu'elle vaut : la plus confortable quand elle marche, inutilisable quand Spotify ferme sa
porte.

---

## ADR-039 — `xargs -a` n'existe pas sur macOS

**Contexte.** La commande générée pour récupérer les manquants répondait
`xargs: invalid option -- a` sur la machine de l'utilisateur.

**La cause.** `-a` est une **extension GNU**. La version BSD livrée avec macOS ne la connaît
pas. Une application macOS générant une commande pour macOS n'avait aucune excuse de supposer
un `xargs` Linux.

**Décision.** Le détour n'avait même pas lieu d'être : `spotdl` accepte directement un fichier
de requêtes en argument. Et surtout, une **seconde commande** est désormais proposée, en
premier — une boucle `yt-dlp`, qui ne dépend d'aucun accès à Spotify.

**Pourquoi `yt-dlp` est devenu la voie principale.** L'accès Spotify de `spotdl` est cassé par
intermittence (voir ADR-038), y compris pour les recherches textuelles. Or Onzer possède
maintenant sa **propre identification acoustique** : il n'a plus besoin qu'un téléchargeur lui
apporte des métadonnées. Aspirer le son brut suffit.

Le nom de fichier est composé depuis la requête et non depuis le titre de la vidéo — les
titres YouTube sont bruités (« [Clip Officiel] », « prod. by … ») alors que la requête est
déjà propre. On obtient « Artiste - Titre.mp3 », soit exactement ce que le filet de sécurité
de l'ouvrier sait relire. Les barres obliques sont neutralisées : « AC/DC » créerait sinon un
dossier au milieu du chemin.

---

## ADR-040 — Le nom du fichier est le plan B de l'empreinte

**Contexte.** L'empreinte acoustique échoue sur environ un fichier sur trois quand la source
est un clip vidéo : intro parlée, jingle, outro — le signal est décalé au point que l'index
ne reconnaît plus rien. Le morceau restait alors sans pochette ni album, alors que son nom de
fichier disait exactement de quoi il s'agissait.

**Décision.** Quand AcoustID ne trouve rien, l'ouvrier cherche dans MusicBrainz **par le
texte** — artiste et titre lus dans le nom du fichier.

**Ce qui ne change pas : la corroboration.** La correspondance textuelle passe par le
**même juge** que l'acoustique (`verdict`) — durée compatible, tags non contredits. Un filet
de sécurité qui accepterait n'importe quoi serait pire que pas de filet : il écrirait de faux
tags là où il n'y en avait aucun.

Deux détails : le score MusicBrainz (0 à 100) est ramené sur l'échelle d'AcoustID (0 à 1)
pour que les deux passent par le même juge sans conversion ad hoc ; et les caractères
réservés de Lucene sont neutralisés — un titre contenant une parenthèse ferait répondre une
erreur de syntaxe plutôt qu'une liste vide.

---

## ADR-041 — Une touche qui ne répond pas doit dire pourquoi

**Contexte.** Les touches F7, F8 et F9 d'un clavier Apple émettent des événements **système**,
pas des frappes ordinaires : la page web ne les voit jamais, et l'API `MediaSession` ne les
capterait pas davantage — elle suppose que le son sort de la page, alors qu'il sort du moteur
Rust. Seul un raccourci global peut les recevoir.

**Le mur.** L'enregistrement échoue sur macOS tant que l'application n'a pas l'autorisation
d'accessibilité : `Failed to watch media key event`. Sans rien de plus, l'utilisateur appuie
sur F8, rien ne se passe, et rien ne lui dit pourquoi.

**Décision.** L'échec est **conservé et affiché** sur la page des raccourcis, avec le chemin
exact du réglage à ouvrir et un bouton « J'ai autorisé, réessayer » qui rejoue
l'enregistrement sans redémarrage. C'est la leçon de l'ADR-030 appliquée d'avance : un
avertissement qui ne quitte pas les journaux est un défaut qui attend.

**La page des raccourcis** existe pour la même raison qu'un aide-mémoire : un raccourci ne
s'apprend pas au moment où l'on en a besoin — à ce moment-là, on a déjà pris la souris.

---

## ADR-042 — Un état qui existe doit être compté quelque part

**Contexte.** L'identification semblait **bloquée à 550 sur 574**. Rien ne progressait plus,
aucune erreur n'apparaissait, et la file d'attente était vide.

**Le diagnostic.** Rien n'était bloqué. Le décompte réel : 299 identifiés, 251 introuvables,
**24 rejetés** — 574 au total. L'état `rejected`, introduit avec la corroboration
(ADR-034), n'avait jamais été ajouté à la requête de progression. Les morceaux qu'il capturait
disparaissaient donc du total affiché : ni en attente, ni terminés, nulle part.

**Décision.** `IdentificationProgress` porte désormais un champ `rejected`, et la jauge
affiche « · N à vérifier ». La règle qui en découle vaut au-delà de ce cas : **tout état
d'une machine à états doit apparaître dans le décompte qui la résume**, sans quoi la somme
ment et l'utilisateur conclut à un blocage. Un test vérifie que les états se totalisent.

---

## ADR-043 — Une correspondance exacte n'est pas une correspondance trouvée

**Contexte.** Les paroles étaient rarement récupérées. Le soupçon initial visait LRCLIB.

**La mesure.** Sur dix morceaux de la bibliothèque, `/api/get` — la route à correspondance
stricte — en trouvait **trois**. La route `/api/search`, sur les dix mêmes, en trouvait
**huit**. Le défaut n'était pas dans le service : il était dans mon appel, qui exigeait une
égalité de durée à la seconde près, alors que la même chanson diffère de deux secondes selon
l'encodage.

**Décision.** `fetch()` tente `/api/get`, puis **retombe sur `/api/search`**. La tolérance de
douze secondes remplace l'égalité stricte, et parmi les résultats acceptables, les paroles
synchronisées passent devant les autres. Le garde-fou demeure : une durée trop éloignée est
refusée, car des paroles fausses sont pires que pas de paroles (ADR-033).

---

## ADR-044 — Une archive bénévole ne remplace pas un catalogue

**Contexte.** 165 morceaux sur 574 restaient sans pochette après le passage MusicBrainz →
Cover Art Archive.

**La mesure.** La Cover Art Archive est alimentée à la main, œuvre par œuvre : excellente sur
les catalogues anciens, lacunaire ailleurs. Sur huit morceaux tirés au hasard parmi les
manquants, **Deezer en a trouvé huit**. iTunes, testé en parallèle, rendait « Polokus » pour
« Damso — Macarena » : sa recherche mélange les champs, celle de Deezer les respecte
(`artist:"…" track:"…"`).

**Décision.** MusicBrainz reste **en premier** — il apporte l'album et l'année en plus de
l'image, donc davantage de savoir. Deezer intervient **en second**, uniquement pour l'image,
et sous le même contrôle de durée que partout ailleurs : quinze secondes d'écart maximum,
sans quoi c'est une reprise et l'image serait fausse. L'appel est anonyme, sans clé ni compte.

---

## ADR-045 — Les deux commandes, toujours, sans avoir à demander

**Contexte.** Onzer sait nommer ce qui manque — les morceaux d'une playlist comparée
(ADR-038) comme les recommandations. Il proposait jusqu'ici une seule commande, `yt-dlp`.

**Le constat de l'utilisateur.** `spotdl` rend directement les métadonnées correctes et la
pochette ; `yt-dlp` rend le son brut mais ne dépend d'aucun accès à Spotify — celui-ci
tombant régulièrement (ADR-036). Les deux ne se remplacent pas : **ils s'enchaînent**.

**Décision.** Les deux commandes sont générées **partout où une liste est produite**, dans
cet ordre : `spotdl` d'abord, puisqu'il ne laisse rien à rattraper quand il fonctionne ;
`yt-dlp` ensuite, pour repasser sur ce que le premier n'a pas obtenu. La syntaxe est celle de
macOS, vérifiée (ADR-039). Onzer n'exécute ni l'une ni l'autre : il les met en forme,
l'utilisateur décide.

---

## ADR-046 — La personnalisation ne doit pas contaminer les données

**Contexte.** Une bibliothèque qu'on ne peut pas marquer de sa main reste un inventaire.
Manquaient : une image de playlist, une description, et un endroit à soi.

**Décision.** Trois ajouts, séparés par nature :

| Ajout | Où il vit | Ce qu'il touche |
|---|---|---|
| Image de playlist | `playlists.cover_path` | Prime sur la mosaïque déduite, ne modifie aucun fichier |
| Description | `playlists.description` | Idem |
| **Note personnelle** sur un morceau | `tracks.note` (migration 0005) | Rien d'autre |

La note est la trouvaille assumée : un souvenir attaché à une chanson — où on l'a entendue,
à qui elle fait penser. Elle n'est **jamais** écrite dans les tags du fichier, **jamais**
transmise au moteur de recommandation, **jamais** comptée dans les statistiques. Une note qui
influencerait ce qu'Onzer propose cesserait d'être un espace libre : on se mettrait à écrire
pour la machine. Elle reste dans la base, à l'utilisateur seul.

---

## ADR-047 — Un catalogue n'est pas une archive, et deux valent mieux qu'un

**Contexte.** L'empreinte acoustique laisse deux restes : 256 morceaux introuvables et
93 sans album — un **tiret** dans l'interface, et une absence dans la vue par albums.

**La mesure.** Sur quatorze de ces morceaux, chaque service pris seul en retrouve six.
Ensemble, sept — les trous ne sont pas aux mêmes endroits. Un exemple de chaque côté :
Deezer trouve « Macarena » que MusicBrainz ne rattachait qu'à des compilations ; iTunes
trouve « Drapeau noir » que Deezer manquait.

**Décision.** Trois catalogues, une seule règle de notation (`identify::catalog`), et deux
portes distinctes :

| Porte | Qui décide | Seuils |
|---|---|---|
| Automatique | La machine | Titre ≥ 0,85 **et** artiste ≥ 0,8 **et** durée à ±30 s |
| Manuelle | L'utilisateur | Aucun — la liste montre tout, triée |

Le reproche fait à iTunes en ADR-044 — il rendait « Polokus » pour « Macarena » — visait une
décision **prise sans témoin**. Il ne vaut plus quand un humain regarde la liste : une
proposition de trop ne coûte qu'un coup d'œil. iTunes revient donc, mais **par la porte
manuelle uniquement**.

**Résultat mesuré** : 56 albums sur 91 complétés automatiquement. Les 35 restants sont
presque tous des fichiers qui contiennent un projet entier — quatorze, vingt, quarante-sept
minutes — qu'aucun catalogue ne peut reconnaître comme un morceau de trois minutes.

---

## ADR-048 — Le titre identifie, la durée corrobore

**Contexte.** La recherche par champs `artist:"…" track:"…"` paraissait la plus sûre.

**Les deux mesures qui l'ont défaite.**

| Ce qu'on croyait | Ce qu'on a mesuré |
|---|---|
| La requête par champs est plus précise | Elle manque « Υ. 2 DIAMANTS » (Deezer l'écrit avec un upsilon grec) et « L'étrangère » — le texte libre les trouve, durée exacte à l'appui |
| La durée suffit à écarter les erreurs | Sur « Drapeau noir », le texte libre remonte trois autres titres du **même album**, dont deux passent le filtre de durée — et le bon, lui, dure 19 s de moins |

**Décision.** On interroge en texte libre et on trie soi-même, par **ressemblance des titres
d'abord** (coefficient de Dice sur les mots, après retrait des « (feat. …) » et autres
mentions de production), la durée ne servant qu'à départager des fiches déjà plausibles. La
tolérance passe à trente secondes : un fichier téléchargé porte souvent une intro que la
version commerciale n'a pas.

Filtrer sur la seule durée aurait attaché « L'insolence des élus » à « Drapeau noir ». C'est
le sens de la règle : **le titre identifie, la durée corrobore — jamais l'inverse.**

---

## ADR-049 — Une commande qui s'arrête au premier échec n'est pas une commande

**Contexte.** `spotdl` sait lire un fichier de requêtes. Sur trente recommandations, une
seule était introuvable — « Bush — Machinehead ». `LookupError`, processus terminé,
**vingt-neuf morceaux jamais tentés**.

**Le piège suivant.** Le réflexe serait `commande || consigner_l_echec`. Mesuré : `spotdl`
sort avec le **code 0 même quand il échoue** — une requête finissant sur `AudioProviderError`
rend exactement le même code qu'un téléchargement réussi. Le `||` ne se déclencherait jamais.

**Décision.** Une invocation **par requête**, pour qu'un échec ne fasse tomber que sa propre
ligne ; et un constat d'échec fondé sur le seul témoin fiable — **un fichier est apparu dans
le dossier, ou il n'est pas apparu**. Les requêtes sans fichier sont consignées dans
`_echecs.txt`, que la commande `yt-dlp` reprend d'elle-même si elle le trouve, et ignore
sinon. Les deux passes s'enchaînent sans rien redemander à l'utilisateur.

Vérifié de bout en bout sur le cas exact rencontré : un faux `spotdl` qui échoue sur la
première ligne, les deux suivantes récupérées, `_echecs.txt` contenant la seule ligne perdue,
et la passe `yt-dlp` la reprenant.

---

## ADR-050 — Le webview ne parle à personne, pas même pour une vignette

**Contexte.** Le choix manuel affiche les pochettes proposées. Les adresses sont chez Deezer
et Apple ; la politique de sécurité du contenu (ADR-005) n'autorise que `self`, `asset:` et
`data:`.

**Le choix.** Élargir la politique à ces deux domaines aurait suffi, en une ligne.

**Décision.** Non : les octets passent par le cœur Rust et arrivent en `data:` URI. La
politique reste telle quelle. Ce n'est pas une précaution abstraite — c'est ce qui garantit
que l'interface d'un lecteur hors ligne **ne peut pas** émettre de requête, quoi qu'y injecte
un jour un titre de morceau ou une réponse de service. La commande n'accepte d'ailleurs que
les domaines d'images des catalogues connus : elle ne doit pas devenir un passe-partout.

Les images ont leur propre file d'attente (`identify::images`), séparée de celle des API :
la cadence de trois secondes et demie d'iTunes protège son API, pas son serveur d'images.

---

## ADR-051 — Une fiche retenue s'écrit d'un bloc

**Contexte.** Appliquer une fiche choisie pourrait n'écrire que ce qui manquait — l'album,
par exemple, en laissant le titre du fichier.

**Décision.** Non. Choisir une fiche, c'est dire « ce morceau est celui-là » : titre, artiste,
album, année et pochette sont écrits ensemble. Un état mixte, moitié fichier moitié catalogue,
serait impossible à expliquer trois mois plus tard.

Deux conséquences en découlent, toutes deux volontaires :

- les **paroles sont effacées** si le titre change, comme pour la correction manuelle
  (ADR-046) : elles appartenaient à l'ancien titre ;
- l'état d'identification passe à `rejected` avec une note qui dit la vérité — « fiche Deezer
  retenue à la main » — pour que l'ouvrier ne repasse jamais par-dessus un choix humain.

La complétion automatique des albums, elle, n'écrit **que** l'album, l'année et la pochette :
elle ne choisit pas une identité, elle comble un vide.

---

## ADR-052 — Une liste qui s'arrête sans le dire ment sur la bibliothèque

**Contexte.** La bibliothèque chargeait cinq cents morceaux et s'arrêtait là. Avec 590
morceaux importés, quatre-vingt-dix n'apparaissaient nulle part — sans message, sans bouton,
sans rien. Le silence est ici pire qu'une erreur : il laisse croire que l'import a échoué.

**Décision.** Cinq cents n'est plus un plafond mais un **pas**. Une sentinelle invisible en
fin de liste demande la tranche suivante dès qu'elle approche du champ de vision, avec 400 px
d'anticipation — le chargement ne se voit pas.

L'ordre de la requête, `added_at DESC, id DESC`, est **stable** : la pagination par décalage
ne peut ni sauter une ligne ni en montrer deux fois. Vérifié sur la bibliothèque réelle :
500 + 90 = 590, aucun recouvrement.

**Pourquoi un observateur d'intersection et non un `onScroll`.** Écouter le défilement
obligerait la table à connaître le conteneur qui défile — une dépendance qui casserait au
premier remaniement de la mise en page. L'observateur ne demande rien à personne.

**Ce qui reste à faire.** Le rendu n'est pas virtualisé : au-delà de quelques milliers de
lignes, le coût sera dans le DOM et non dans la base. Le jour venu, seule la table changera —
la pagination, elle, est déjà en place.

---

## ADR-053 — Un problème résolu ne se fait pas confirmer

**Contexte.** Le panneau des doublons propose « Ce ne sont pas des doublons » pour écarter un
groupe légitime. Mais quand on retirait les morceaux un par un jusqu'à n'en laisser qu'un, le
groupe restait affiché — et réclamait encore ce clic.

**Décision.** Un groupe retombé à un seul morceau n'est plus un doublon : il disparaît de
lui-même. La requête l'excluait déjà ; c'est l'état local, entre deux relectures, qui le
gardait à l'écran. Or c'est cet état-là que l'utilisateur regarde.

La règle vaut au-delà de ce panneau : **une question dont la réponse est devenue évidente ne
doit plus être posée.** Demander confirmation d'un problème déjà réglé donne l'impression que
l'application n'a pas compris ce qui vient de se passer.

---

## ADR-054 — Une copie qui ne suit pas est un mensonge indexé

**Contexte.** L'index de recherche FTS5 duplique le titre, les artistes et l'album : c'est le
prix de la recherche instantanée. Toute copie dérive dès qu'un seul chemin d'écriture
l'oublie.

**La mesure.** 113 morceaux sur 590 portaient dans l'index un album qu'ils n'avaient plus.
« Macarena » répondait encore à « I migliori anni '90 » — la compilation italienne détachée
depuis (ADR-032) — et restait introuvable sous son vrai album. Rattacher une pochette
changeait l'album en base sans toucher à l'index ; la complétion automatique et la révision
faisaient de même.

**Décision.** Une seule fonction, `reindex_track`, appelée par tous les chemins qui touchent
à l'identité ou à l'album. Elle **relit la base** au lieu de recevoir les valeurs en
paramètre : un appelant qui se tromperait propagerait son erreur au lieu de la corriger.

Et, comme toujours, corriger le chemin ne répare pas ce qu'il a déjà écrit : la remise en
état au démarrage compare index et base, et ne réécrit que les lignes divergentes. Vérifié
de bout en bout sur la bibliothèque réelle — une divergence introduite à la main est
détectée, signalée et corrigée au lancement suivant.

**Pourquoi ne pas tout réindexer au démarrage.** Réécrire 590 lignes chaque fois coûterait
pour rien et rendrait le rapport illisible : il annoncerait 590 réparations là où il n'y a
rien à réparer. Un rapport qui crie tout le temps ne se lit plus.

---

## ADR-055 — Un outil devenu inutile se range, il ne se jette pas

**Contexte.** Onzer sait combler ce qui manque : identifier à l'oreille, chercher paroles,
pochettes et albums. Ces outils ont été écrits pour une bibliothèque bâtie au fil de
téléchargements approximatifs. Le jour où les fichiers arrivent déjà tagués et pochettés,
les mêmes bandeaux deviennent du bruit — quatre encarts qui proposent de réparer ce qui n'est
pas cassé.

**Décision.** Un **réglage**, pas une suppression. Par défaut la complétion reste active,
comme au premier jour : une application découverte par quelqu'un d'autre doit savoir réparer
une bibliothèque en désordre, puisque c'est le cas le plus probable. Qui n'en a plus besoin
l'éteint, et l'interface se tait.

**Ce que le réglage éteint, et ce qu'il n'éteint pas.** Il gouverne ce qu'Onzer entreprend
**de lui-même** : l'ouvrier d'identification et les passes qui traitent toute la bibliothèque
d'un coup. Il ne touche pas aux gestes posés explicitement sur un morceau — « Chercher en
ligne », « Chercher ailleurs ». Refuser ceux-là ne protégerait de rien : personne n'est
sollicité, c'est l'utilisateur qui demande. **Le réglage fait taire une proposition, il ne
confisque pas un outil.**

**Le cœur refuse aussi.** Masquer un bouton n'empêche personne d'appeler la commande qu'il
déclenche. Un réglage qui ne tient que par ce que l'on affiche n'est pas un réglage, c'est
une décoration.

---

## ADR-056 — Marquer supprimé ne suffit pas quand la place doit être rendue

**Contexte.** Remplacer toute une bibliothèque par de meilleurs fichiers suppose de vider
l'ancienne. Le réflexe — marquer les lignes supprimées, comme partout ailleurs dans Onzer —
échoue ici pour une raison précise :

`tracks.relative_path` est **UNIQUE**, y compris pour une ligne marquée supprimée. Le même
morceau retéléchargé retomberait sur `Damso/Ipséité/03 - Macarena.mp3`, et la base refuserait
l'insertion. La bibliothèque paraîtrait vide, l'import échouerait, et rien n'expliquerait
pourquoi.

**Décision.** Les champs uniques des lignes écartées sont **neutralisés** :

```text
  relative_path : Damso/Ipséité/03 - Macarena.mp3  →  ancien:412
  content_hash  : 9f3c…                            →  ancien:412
  audio_hash    : 7a10…                            →  NULL
```

La place est rendue, et plus aucune de ces lignes ne peut être confondue avec un fichier réel
par le dédoublonnage à l'import.

**Ce qui survit.**

| Donnée | Sort | Raison |
|---|---|---|
| Historique d'écoute | Conservé | C'est la matière des statistiques, et il ne se reconstitue pas |
| Playlists — nom, image, description | Conservées | Ce sont des créations, pas des données de fichiers |
| Contenu des playlists | Vidé | Ce qu'elles pointaient n'existe plus |
| Fichiers audio | Mis de côté | Onzer ne détruit rien sur le disque (ADR-007) |

**Pourquoi déplacer des dossiers et non des fichiers.** Six cents renommages là où une
vingtaine suffisent — et sur le même volume, déplacer un dossier est instantané quel que soit
son poids. Surtout, la hiérarchie est conservée : qui ouvre `_Ancien` dans le Finder retrouve
sa bibliothèque telle qu'il l'avait laissée, et non six cents fichiers en vrac. Le dépôt
`_Inbox` est épargné : c'est là que les nouveaux fichiers attendent.

**L'ordre compte.** La base d'abord, le disque ensuite. Si le déplacement échoue à mi-chemin,
la base est déjà cohérente et un nouvel import repartira proprement ; l'inverse laisserait des
lignes pointant vers des fichiers absents.

---

## ADR-057 — « A des paroles » n'est pas « a des paroles qui défilent »

**Contexte.** Après le passage à deemix, 1378 morceaux sur 1711 portaient leurs paroles.
**Aucune** n'était synchronisée. Et la passe de récupération n'a jamais rien proposé : sa
question était « ce morceau a-t-il des paroles ? », et la réponse était oui.

**Le défaut.** Confondre deux états distincts sous un même compteur. Un morceau au texte brut
n'est pas fini, il est **à moitié fait** — mais il était rangé avec les morceaux terminés, donc
invisible. Même famille de bogue que l'ADR-042 : un état qui existe et que personne ne compte
disparaît, et l'utilisateur en conclut que l'outil ne sert à rien.

**Décision.** La synchronisation est comptée à part, en base comme à l'écran :

| Ce qu'on demandait | Ce qu'on demande |
|---|---|
| `lyrics IS NOT NULL` | `lyrics LIKE '%[__:__%'` |
| « 0 morceau sans paroles » | « 1378 morceaux sans synchronisation » |

La passe vise désormais les deux manques, et **ne remplace jamais un texte brut par un autre
texte brut** : réécrire mille fichiers pour changer une version non horodatée contre une autre
ne rendrait service à personne.

---

## ADR-058 — Une correspondance exacte non synchronisée ne clôt pas la recherche

**Contexte.** `fetch` interrogeait la route stricte de LRCLIB, puis se rabattait sur la
recherche large (ADR-043). Mais dès que la route stricte rendait *quelque chose*, elle
s'arrêtait — y compris quand ce quelque chose était du texte brut.

**La mesure.** Sur quinze morceaux tirés au hasard de la bibliothèque :

| Chemin | Versions synchronisées trouvées |
|---|---|
| Route stricte seule | 9 |
| Stricte, puis large si non synchronisée | **13** |

Trois morceaux — « BLASE », « Magic », « infinity (888) » — n'existaient en version horodatée
que sous une autre fiche. S'arrêter à la première réponse coûtait un cinquième des paroles
récupérables.

**Décision.** La **synchronisation prime sur l'exactitude de l'appariement**. À forme égale, la
correspondance exacte l'emporte, puisqu'elle a le plus de chances de désigner le bon
enregistrement. La règle est isolée dans une fonction pure, `prefer_synced`, testable sans
réseau.

---

## ADR-059 — Le fichier posé à côté prime sur le tag

**Contexte.** Le format `USLT` d'ID3 ne prévoit pas d'horodatage. Les téléchargeurs qui savent
produire des paroles synchronisées ne les écrivent donc pas dans les tags : ils déposent un
fichier `.lrc` du même nom à côté du morceau.

**Décision.** À la lecture, un `.lrc` **synchronisé** posé à côté du fichier l'emporte sur les
paroles du tag. S'il existe, c'est qu'il apporte ce que le tag ne sait pas porter — et il est
là, sur le disque, sans réseau et sans attente.

Un `.lrc` non synchronisé, lui, ne prend pas la place du tag : il n'apporterait rien de plus.

---

## ADR-060 — La synchronisation était déjà dans les fichiers

**Contexte.** L'ADR-057 corrigeait le décompte, l'ADR-058 la recherche : il ne restait plus,
croyait-on, qu'à interroger LRCLIB pour mille trois cents morceaux. Un quart d'heure de
requêtes réseau pour une bibliothèque qu'on venait de télécharger.

**La découverte.** ID3 possède **deux** trames de paroles :

| Trame | Contenu | Ce qu'Onzer lisait |
|---|---|---|
| `USLT` | Le texte, sans horodatage | ✅ |
| `SYLT` | Le texte **horodaté**, ligne par ligne | ❌ |

`ItemKey::Lyrics`, l'abstraction de lofty, désigne `USLT`. Onzer ne lisait donc que le texte
brut — et concluait que les fichiers n'étaient pas synchronisés, alors que la synchronisation
était là, dans la trame d'à côté, écrite par le téléchargeur.

Mesuré sur quatre-vingts fichiers tirés au hasard : **cinquante portaient une trame `SYLT`**.
Près des deux tiers de ce qu'on s'apprêtait à demander à un service public était déjà sur le
disque de l'utilisateur.

**Décision.** La passe de synchronisation va **du plus proche au plus lointain** :

```text
  1. trame SYLT du fichier      → gratuit, instantané, hors ligne
  2. fichier .lrc posé à côté   → idem (ADR-059)
  3. LRCLIB                     → pour ce qui reste, si autorisé
```

La première passe fonctionne **même quand la complétion en ligne est éteinte** : elle ne parle
à personne, elle relit des fichiers que l'utilisateur possède. Et elle n'écrit qu'en base :
réécrire ces paroles dans le fichier reviendrait à lui rendre ce qu'il vient de nous donner,
en remaniant mille blocs de tags pour rien.

**La leçon, qui dépasse les paroles.** Avant d'aller demander dehors, épuiser ce qu'on a chez
soi. L'abstraction d'une bibliothèque — ici `ItemKey::Lyrics` — décrit ce que son auteur a
jugé commun à tous les formats ; elle ne décrit pas ce que le fichier contient. Quand une
donnée « manque » partout à la fois, se demander d'abord si on la cherche au bon endroit.

---

## ADR-061 — Deux interrupteurs, parce que ce sont deux questions

**Contexte.** Un seul réglage gouvernait « la complétion en ligne » : paroles, pochettes,
albums **et** l'ouvrier d'identification acoustique. Demander des paroles réveillait donc un
ouvrier qui réécrit des titres — un effet de bord que rien n'annonçait.

**Décision.** Deux réglages distincts, gardés chacun côté cœur comme côté interface. Les deux
ressemblent à une seule question — « Onzer a-t-il le droit d'aller en ligne ? » — mais
répondent à des besoins opposés : vouloir des paroles sur une bibliothèque bien taguée est
courant, vouloir qu'on en réécrive les titres ne l'est pas.

---

## ADR-062 — Le modèle donne la minuterie, jamais les mots

**Contexte.** Après le disque et LRCLIB, 260 morceaux gardent leurs **vraies** paroles sans
horodatage, et 276 n'ont rien du tout.

**Le choix qu'on aurait pu faire, et qui aurait été mauvais.** Afficher directement la
transcription du modèle. Sur du rap français, Whisper se trompe de mot régulièrement :
l'utilisateur verrait défiler un texte approximatif à la place de paroles exactes qu'il
possédait déjà.

**Décision.** Un **alignement**, pas une substitution :

```text
  paroles officielles     transcription du modèle
  « J'm'arrache à lire »  « [00:21.8] je marrache alire »
           │                          │
           └──────────  ALIGNEMENT  ──┘
                         │
           « [00:21.8] J'm'arrache à lire »
             ↑ les mots officiels, la minuterie du modèle
```

Un Needleman-Wunsch sur les mots normalisés — l'algorithme des séquences biologiques, qui
tolère qu'un mot manque ou soit mal entendu là où une comparaison ligne à ligne se perdrait au
premier décalage. La pire erreur possible devient un décalage de quelques secondes ; **jamais
un mot inventé**.

**Le détail mesuré qui a changé l'algorithme.** Sur « HS 808 », dont l'introduction est chantée
deux fois, l'alignement retenait la **seconde** occurrence — les scores étant identiques, rien
ne départageait. La première ligne s'affichait cinq secondes trop tard. La remontée préfère
désormais le trou à l'appariement **à égalité stricte**, ce qui revient à choisir toujours le
passage le plus tôt : `[00:05.26]` est devenu `[00:00.02]`.

**Les morceaux sans aucune parole** sont transcrits — c'est le modèle qui écrit — et le
résultat porte une signature `[by:Onzer — transcription automatique]` : une métadonnée que le
format `.lrc` prévoit, que l'analyseur ignore déjà et que tout autre lecteur affichera. C'est
une option **décochée par défaut** : deviner un texte et le présenter comme officiel serait
exactement le genre de service qu'on ne rend pas.

**Ce qui ne change pas.** Le modèle tourne sur la machine, sur un fichier du disque. Rien ne
sort : ni l'audio, ni le texte, ni le fait qu'une transcription ait eu lieu. C'est la seule
forme d'intelligence artificielle qu'un lecteur hors ligne puisse honnêtement proposer — et
c'est aussi pourquoi ce bandeau reste visible quand la complétion en ligne est éteinte.

**Mesuré de bout en bout** sur un morceau de 2 min 20 : 292 mots entendus en 40 s sur un M4,
52 lignes calées, texte officiel intact.

---

## ADR-063 — Un morceau sans fichier n'est pas un doublon

**Contexte.** 154 morceaux avaient perdu leur fichier. Retéléchargés et déposés au dépôt, ils
arrivaient avec les mêmes tags — la détection de doublon les reconnaissait, et les **écartait**.

Le résultat, du point de vue de l'utilisateur : « Été avec toi » reste grisé, injouable, sans
qu'aucune action ne soit possible ; et le même titre apparaît **deux fois** dans le dossier des
doublons. Quatre cent sept fichiers s'y étaient accumulés.

**Décision.** Avant de conclure au doublon, on demande **au disque** si le morceau reconnu a
encore son fichier. S'il ne l'a plus, l'import ne crée pas de ligne : il **rend son fichier à
la ligne existante**.

| | Nouvel import | Retrouvailles |
|---|---|---|
| Titre, artiste, album | nouveaux | **inchangés** |
| Historique, favoris, playlists | vides | **conservés** |
| Chemin, taille, empreintes | écrits | **réécrits** |

**Pourquoi le disque et non `is_available`.** Cette colonne ne vaut que ce que vaut le dernier
balayage. Se fier à elle écraserait le chemin d'un morceau parfaitement présent — le seul
endroit de tout ce mécanisme où une erreur ferait perdre quelque chose.

**Et les 407 déjà écartés.** Corriger une règle ne répare pas ce qu'elle a déjà fait
(ADR-029). Un bouton « Reprendre les fichiers écartés » rejoue l'import sur `_Doublons` : ce
qui correspond à un morceau sans fichier le rejoint, les vrais doublons ne bougent pas.

---

## ADR-064 — Le coût avait changé de camp

**Contexte.** L'ADR-052 avait supprimé le plafond de cinq cents morceaux par un chargement à
la volée. La question de la **base** était réglée ; celle du **navigateur** ne l'était pas. À
deux mille lignes réellement dessinées — chacune avec son menu, ses boutons et ses infobulles
— le défilement saccade.

**Décision.** Des pages de cent. Pas un compromis en attendant mieux : cent lignes tiennent
largement plus d'un écran, se dessinent instantanément, et donnent un repère que le
défilement infini n'offre jamais — « page 3 sur 21 » dit d'un coup d'œil où l'on est et ce
qu'il reste.

C'est la solution que le problème appelait. Virtualiser le rendu — ne dessiner que les lignes
visibles d'une liste infinie — aurait ajouté un mécanisme délicat (hauteurs mesurées, ancrage
du défilement, accessibilité du clavier) pour résoudre un problème que la pagination fait
disparaître.

**Le défaut que ce changement a révélé.** Les playlists générées habillaient leurs
identifiants en piochant dans la première page de la bibliothèque. Tant que la page couvrait
tout, cela marchait ; en la réduisant à cent, tout ce qui se trouvait au-delà aurait
**disparu de la playlist sans erreur ni message**. La commande `tracks_by_ids` remplace ce
détour : on demande les morceaux qu'on veut, dans l'ordre où on les veut.

Une taille de page n'est jamais qu'un réglage d'affichage — sauf quand du code s'en sert
comme d'une garantie de couverture.

---

## ADR-065 — Android : le cœur passe, c'est le pourtour qui résiste

**Contexte.** Le pari de l'architecture depuis le premier jour : un cœur Rust portable, une
interface web, et rien de spécifique à macOS dans le métier. Première vérification réelle.

**Ce que la compilation a dit.** Sur l'ensemble du cœur — base SQLite, scan, empreintes
acoustiques, recommandation, statistiques, paroles, alignement, serveur d'import —
**une seule erreur** : le greffon des raccourcis globaux, qui n'existe pas sur mobile. Même
`rodio` et `cpal` compilent, ce dernier basculant tout seul sur AAudio, l'interface audio
native d'Android.

Le pari tient. Ce qui a résisté était ailleurs, et n'avait rien à voir avec le code.

**Les quatre murs, dans l'ordre où ils sont tombés.**

| Mur | Cause | Décision |
|---|---|---|
| `unresolved import tauri_plugin_global_shortcut` | Les touches multimédia n'existent pas sur mobile | Greffon et code derrière `#[cfg(desktop)]`, dépendance sous `[target.'cfg(not(android))']` |
| `unable to find library -laaudio` | AAudio n'existe qu'à partir d'Android 8.0 | `minSdkVersion: 26` dans `tauri.conf.json` — le seul endroit qui pilote **à la fois** Gradle et le nom du lieur (`aarch64-linux-android26-clang`) |
| `Unsupported class file major version 69` | Gradle 8.14 ne lit pas les classes d'un JDK 25 | JDK 21 installé dans `~/.jdks`, sans droits administrateur, et figé dans les scripts |
| `Unable to delete … ._shrunk-classpath-snapshot.bin` | **exFAT** | Les sorties Gradle quittent le SSD |

**Le dernier mérite d'être expliqué.** Le projet vit sur exFAT, seul format que macOS et
Android partagent. exFAT ne connaît pas les attributs étendus : macOS les range dans des
fichiers jumeaux `._quelquechose`, invisibles au Finder et bien réels pour tout le reste. Ils
avaient déjà cassé `git push` ; ici ils cassaient Gradle de deux façons, en se faisant passer
pour des classes et en refusant d'être supprimés.

Les dossiers de compilation Gradle partent donc sur le volume APFS qui héberge déjà le cache
Rust — même raisonnement, même endroit, et rien de ce qui s'y écrit n'est une source.

**Ce qui reste local et non versionné.** `src-tauri/gen/` est généré et ignoré par git. Deux
réglages y vivent, propres à cette machine, à réappliquer après un `tauri android init` :
le chemin du JDK 21 dans `gradle.properties`, et la relocalisation des dossiers de sortie dans
`build.gradle.kts` et `buildSrc/build.gradle.kts`. Le reste — niveau d'API compris — est dans
`tauri.conf.json`, donc versionné.

**Où l'on en est.** L'APK se construit. Ce qui vient ensuite est la partie que l'estimation
annonçait comme la vraie : l'interface pensée pour un écran de téléphone, et surtout la
lecture en arrière-plan, qui demande un service de premier plan Android et n'a aucun
équivalent dans ce qui existe aujourd'hui.

---

## ADR-066 — Sur un écran étroit, la navigation descend

**Contexte.** L'interface de bureau tient en trois colonnes : barre latérale de 240 px,
contenu, panneau de droite de 350 px. Sur un téléphone de 375 px de large, la seule barre
latérale mangerait les deux tiers.

**Décision.** La bascule tient à la **largeur**, pas au système. Une fenêtre de bureau
rétrécie a exactement le même problème qu'un téléphone, et la même réponse lui convient —
c'est aussi ce qui rend la disposition mobile vérifiable sur le Mac, en tirant sur un coin de
fenêtre.

En dessous de 768 px : barre latérale et panneau disparaissent, une **barre d'onglets** prend
place en bas, et le lecteur se réduit à ce qui tient au-dessus d'elle.

| Élément | Bureau | Étroit |
|---|---|---|
| Navigation | Barre latérale | Cinq onglets en bas, sous le pouce |
| Lecteur | Dix commandes | Pochette, titre, lecture, suivant |
| Recherche | Champ permanent en haut | Un mode, ouvert depuis son onglet |
| Pochette d'en-tête | 208 px, à côté du titre | 160 px, empilée au-dessus |

**Ce qui ne change pas** : les jetons de couleur, la typographie, les formes, les gestes. La
direction artistique est un calque, pas une mise en page — c'est ce qui permet à cette
bascule de tenir en trois composants.

---

## ADR-067 — Android donne l'audio et les fichiers à deux conditions

**Contexte.** Le cœur compilait pour Android depuis l'ADR-065. L'application démarrait,
s'affichait, et ne faisait rien de ce qu'on lui demandait.

**Deux murs, tous deux invisibles depuis les journaux** — le constructeur du téléphone chiffre
`logcat`. Il a fallu brancher les outils de développement de Chrome sur la WebView par l'USB
pour les voir.

**1. « android context was not initialized ».** `cpal` lit la machine virtuelle Java et
l'objet `Context` dans `ndk_context`, un dépôt global que personne ne remplissait. Il ne
rendait pas une erreur pour autant : il **paniquait**, le fil audio mourait sans un mot, et
l'application ne voyait qu'un canal fermé — « le thread audio n'a pas démarré », sans cause.

La panique est désormais rattrapée et sa cause remontée jusqu'à l'écran ; et
`JNI_OnLoad` remplit le contexte au chargement de la bibliothèque, avant que la moindre ligne
de notre code ne tourne. L'ordre n'est plus une question de discipline, c'est une garantie.
Le `Context` vient de `ActivityThread.currentApplication()` : au chargement, aucune activité
n'existe encore.

**2. « Operation not permitted », 2699 fois.** `READ_MEDIA_AUDIO` laisse **lister** les
fichiers, pas les ouvrir ni les déplacer — le stockage cantonné réserve cela à MediaStore. Or
Onzer range les morceaux par artiste, année et album : déplacer des fichiers est le cœur de ce
qu'il fait.

`MANAGE_EXTERNAL_STORAGE` lève la limite. Elle ne s'accorde pas d'un bouton : le système exige
que l'utilisateur l'active dans ses réglages, et l'application l'y emmène directement.

**Résultat mesuré** : 2351 morceaux, 497 artistes, 1127 albums importés sur le téléphone,
aucun échec. Le moteur audio démarre. **Le flux, lui, ne tourne pas encore** : la position
avance par à-coups puis se fige, sans qu'aucun son ne sorte. C'est le point qui reste ouvert,
et c'était le risque annoncé dès l'estimation.

---

## Dette technique assumée

| Sujet | État | Raison |
|---|---|---|
| Génération des types TS depuis Rust (`specta`) | Reportée | La surface IPC se limite à une commande. `specta` n'existe qu'en version *release candidate* ; l'introduire maintenant apporterait de l'instabilité pour un bénéfice nul. Les types de `src/lib/ipc.ts` sont écrits à la main **et commentés comme tels**. À rebrancher dès que les commandes se multiplient. |
| Icône de l'application | Provisoire | Générée par `tools/icon/generate-icon.mjs` (dégradé violet → cyan des jetons de design, sans aucune dépendance). Reproductible et fonctionnelle, mais à remplacer si une identité visuelle définitive est arrêtée. |

---

## Journal des décisions à venir

- [ ] ADR-009 — Architecture du moteur audio (gapless, crossfade, ReplayGain)
- [ ] ADR-010 — Scanner de bibliothèque et lecture des métadonnées
- [ ] ADR-011 — Moteur de recommandation : signaux, scoring, exploration
- [ ] ADR-012 — Contrat d'import externe (dossier surveillé + API locale)
