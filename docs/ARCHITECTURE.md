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

## Journal des décisions à venir

- [ ] ADR-008 — Schéma de base de données et stratégie de migrations
- [ ] ADR-009 — Architecture du moteur audio (gapless, crossfade, ReplayGain)
- [ ] ADR-010 — Moteur de recommandation : signaux, scoring, exploration
- [ ] ADR-011 — Contrat d'import externe (dossier surveillé + API locale)
