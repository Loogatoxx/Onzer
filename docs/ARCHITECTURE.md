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
