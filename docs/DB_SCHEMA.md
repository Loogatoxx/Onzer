# Schéma de base de données — Onzer

> Ce document explique **le pourquoi**. Le **quoi** fait foi dans
> [`src-tauri/migrations/`](../src-tauri/migrations/) : le SQL commenté y est la
> seule source de vérité, jamais recopié ici pour éviter toute divergence.

---

## Le principe fondateur : journal immuable + dérivés

Deux manières de modéliser l'écoute de musique :

| | Approche naïve | Approche retenue |
|---|---|---|
| Modèle | Une colonne `play_count` qu'on incrémente | Un **journal d'événements** append-only |
| Coût | Minimal | Environ 10 Mo par an |
| « Qu'est-ce que j'écoutais le mardi soir en novembre ? » | ❌ Donnée écrasée, perdue à jamais | ✅ Requête simple |
| Changer la formule de score de la reco | ❌ L'IA repart de zéro | ✅ Recalcul sur tout l'historique |

Tout ce qui n'est pas un événement brut — compteurs, scores d'affinité, profils
de contexte — est **dérivé** et donc régénérable. C'est le filet de sécurité de
tout le projet : aucune décision d'algorithme prise aujourd'hui ne peut coûter
des données demain.

---

## Cartographie

```
RÉFÉRENTIEL                 JOURNAL                    DÉRIVÉS
───────────                 ───────                    ───────
artists                     listening_sessions         track_stats
albums          ──────►     play_events        ──────► track_features
tracks   ◄──────────────────────┘                      track_transitions
track_artists                                          context_profiles
genres / track_genres
tracks_fts (FTS5)           PLAYLISTS                  SYSTÈME
                            playlists                  settings
                            playlist_tracks            import_jobs
```

Le sens des flèches est important : le référentiel alimente le journal, le
journal alimente les dérivés. **Jamais l'inverse.**

---

## Les décisions qui portent la recommandation

### `skip_at_ms` — la colonne que presque personne ne stocke

Skipper un morceau à 3 secondes signifie « je déteste ce son ».
Le skipper à 2 minutes signifie « je l'aime, mais pas maintenant ».

Sans cette colonne, les deux cas sont un unique `skip_count` indifférencié, et
la recommandation devient incapable de distinguer un rejet d'un simple
changement d'humeur.

### `source` — la seule boucle de qualité

Chaque événement sait d'où il vient : `library`, `playlist`, `radio`, `reco`,
`search`, `queue`, `shuffle`.

Comparer le taux de complétion des titres proposés par le moteur à celui des
titres choisis à la main est **la seule façon de savoir si l'algorithme est
bon**. Sans cette colonne, on optimise à l'aveugle.

### `was_manual_selection` — l'intention

Aller chercher activement un morceau est un signal d'affinité d'un tout autre
ordre que le laisser passer dans une file. Deux écoutes complètes n'ont pas le
même poids selon ce champ.

### `previous_track_id` — apprendre les enchaînements

Alimente `track_transitions`. C'est la différence entre une radio qui **coule**
et une playlist qui saute du coq à l'âne : on n'apprend pas seulement quels
morceaux sont aimés, mais lesquels vont bien **après** lesquels.

### `output_device` — le contexte physique

Casque = écoute attentive. Enceintes = fond sonore. AirPods en déplacement =
encore autre chose. Trois intentions, trois recommandations.

### `analyzer` + `analyzer_version` — la réanalyse ciblée

Le jour où l'extraction de features s'améliore, une seule requête dit
exactement quels morceaux réanalyser. Sans ces colonnes, il faudrait tout
recalculer, ou pire, mélanger des vecteurs incompatibles.

---

## Les garde-fous, et ce qu'ils empêchent

Tous vérifiés par les tests de `src-tauri/src/db/mod.rs`.

| Garde-fou | Empêche concrètement |
|---|---|
| `TRIGGER trg_play_events_no_delete` | Qu'une requête maladroite efface des années d'historique |
| `play_events.track_id ... ON DELETE RESTRICT` | Qu'une suppression de morceau emporte son historique d'écoute |
| `artists.normalized_name UNIQUE` | Que « A$AP Rocky », « ASAP Rocky » et « asap rocky » deviennent trois artistes |
| `PRAGMA foreign_keys = ON` | Que le modèle relationnel soit purement décoratif (désactivé par défaut dans SQLite) |
| `CHECK` sur les énumérations | Qu'une faute de frappe dans un état applicatif passe silencieusement en base |

---

## Trois états d'absence à ne jamais confondre

C'est la nuance la plus subtile du schéma, et elle vient directement de
l'ADR-006 (bibliothèque sur SSD amovible).

| État | Signification | Réversible | Effet sur l'historique |
|---|---|---|---|
| `is_available = 0` | Fichier temporairement introuvable — SSD débranché, fichier déplacé à la main | ✅ Automatique au rebranchement | Aucun |
| `deleted_at != NULL` | L'utilisateur a retiré le morceau de sa bibliothèque | ✅ Manuel | Aucun — l'historique est conservé |
| Ligne absente | Le morceau n'a jamais existé dans Onzer | — | — |

**Un morceau n'est jamais supprimé physiquement.** Ses écoutes passées
continuent d'alimenter les statistiques et le moteur de recommandation.

---

## Performance : le dimensionnement réel

Pour une bibliothèque de 5 000 morceaux :

| Métrique | Valeur estimée |
|---|---|
| Événements par an | ~100 000 |
| Croissance de la base | ~10 Mo/an |
| Base après 5 ans | ~50 Mo |

C'est **négligeable** pour SQLite. Conséquence directe : **aucune purge n'est
prévue**. L'historique est conservé à vie, ce qui rend possible un véritable
« Wrapped » comparatif dès la deuxième année.

Le cache SQLite est réglé à 32 Mo (`db/mod.rs`) : à cette échelle, la totalité
des index tient en mémoire et toutes les requêtes de statistiques restent
instantanées.

---

## Conventions

- **Horodatages** : entiers, millisecondes Unix **UTC**.
  Exception assumée : `play_events.hour_local` et `weekday` sont en heure
  **locale** et dénormalisés — recalculer un fuseau sur des centaines de
  milliers de lignes à chaque requête de statistiques serait absurde.
- **Booléens** : entiers `0` / `1` (SQLite n'a pas de type booléen).
- **Chemins** : toujours relatifs à la racine de bibliothèque, séparateur `/`.
- **Champs JSON** : `settings.value`, `playlists.rules_json`,
  `import_jobs.metadata_hint`.
- **Migrations** : `NNNN_description.sql`, jamais modifiées après avoir été
  livrées. Toute évolution passe par un nouveau fichier.
