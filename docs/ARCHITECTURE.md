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

## ADR-002 — macOS d'abord, mais cœur portable Android

**Contexte.** La cible immédiate est macOS. Un portage Android est envisagé si le projet
tient ses promesses. Un portage tardif est très coûteux si le cœur s'appuie sur des API
spécifiques à une plateforme.

**Décision.** Aucune API propre à macOS dans le cœur métier. Tout ce qui est
spécifique à une plateforme est isolé derrière une abstraction.

**Conséquences — règles contraignantes :**

| Domaine | Règle | Raison |
|---|---|---|
| Décodage audio | `symphonia` uniquement (pur Rust) | Compile tel quel sur Android. Interdiction d'utiliser AVFoundation / CoreAudio directement. |
| Sortie audio | `cpal` derrière un trait `AudioOutput` | `cpal` cible CoreAudio sur macOS et AAudio/Oboe sur Android. |
| Chemins de fichiers | Toujours via un `PathResolver` centralisé | Android impose un stockage *scopé* : aucun chemin absolu en dur ne survivrait au portage. |
| Base de données | SQLite | Identique sur les deux plateformes. |
| Cœur métier | N'importe jamais le crate `tauri` | Rend le cœur testable hors application, et réutilisable par un futur binaire mobile ou CLI. |

**⚠️ Point de vigilance identifié : l'extraction de features audio.**
La librairie `bliss-audio` (référence pour la similarité musicale en Rust) dépend de **FFmpeg**,
pénible à cross-compiler pour Android. La parade est actée dès maintenant :

```rust
/// Le moteur de recommandation ne connaît QUE ce trait.
/// Il ignore totalement comment le vecteur de features est produit.
trait FeatureExtractor {
    fn extract(&self, path: &Path) -> Result<FeatureVector>;
}
```

- `BlissExtractor` — implémentation desktop, disponible immédiatement.
- `NativeExtractor` — implémentation future 100 % Rust (`symphonia` + `rustfft`) pour Android.

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

## Journal des décisions à venir

- [ ] ADR-006 — Schéma de base de données et stratégie de migrations
- [ ] ADR-007 — Architecture du moteur audio (gapless, crossfade, ReplayGain)
- [ ] ADR-008 — Moteur de recommandation : signaux, scoring, exploration
- [ ] ADR-009 — Contrat d'import externe (dossier surveillé + API locale)
