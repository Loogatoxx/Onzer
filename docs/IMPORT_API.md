# Import automatique — contrat pour scripts externes

> Onzer expose deux façons d'alimenter la bibliothèque sans intervention.
> Ce document est le **contrat stable** sur lequel un script de téléchargement
> peut s'appuyer sans rien connaître du fonctionnement interne d'Onzer.

---

## Les deux voies

```
   dossier _Inbox  ──┐
                     ├──►  même pipeline d'import  ──►  bibliothèque rangée
   API 127.0.0.1   ──┘
```

| | Dossier surveillé | API HTTP locale |
|---|---|---|
| Intégration | Aucune : on dépose un fichier | Quelques lignes de code |
| Métadonnées | Tags du fichier, sinon nom de fichier | **Peut fournir des indications** |
| Éviter un téléchargement inutile | ❌ | ✅ via `/exists` |
| Retour immédiat | ❌ | ✅ `imported` ou `duplicate` |

**Recommandation :** l'API si ton script peut faire une requête HTTP, le dossier
sinon. Les deux peuvent cohabiter.

---

## Voie 1 — le dossier surveillé

Dépose simplement un fichier audio dans :

```
<racine de bibliothèque>/_Inbox/
```

Onzer inspecte ce dossier **toutes les 2 secondes** et importe tout fichier dont
la taille n'a plus bougé depuis **3 secondes**.

**Pourquoi cette attente.** `yt-dlp` et la plupart des téléchargeurs créent le
fichier *avant* de le remplir. Importer dès son apparition avalerait un MP3
tronqué, le taguerait de travers et le rangerait définitivement. Un fichier qui
grossit encore n'est donc jamais touché.

**Ce que tu dois savoir :**
- Le dossier n'est **pas** exploré récursivement.
- Un fichier importé est **déplacé** hors du dépôt.
- Un doublon est **laissé sur place** : à toi de décider quoi en faire.
  Onzer ne supprime jamais rien de lui-même.
- Un fichier dont l'import échoue reste là et n'est **pas** retenté en boucle.
  Le retrouver dans `_Inbox` signale un problème.
- Le dépôt est exclu du scan de bibliothèque : aucun risque d'indexer un
  téléchargement en cours.

---

## Voie 2 — l'API HTTP locale

```
http://127.0.0.1:47811
```

**Écoute uniquement sur la boucle locale.** Rien n'est joignable depuis le
réseau.

### Authentification

Toutes les routes exigent un jeton :

```
Authorization: Bearer <jeton>
```

Le jeton est généré au premier démarrage et lisible ici, par ton seul compte
utilisateur (`chmod 600`) :

```
~/Library/Application Support/com.loogatoxx.onzer/onzer.token
```

> **Pourquoi un jeton alors qu'on écoute en local ?**
> Écouter sur `127.0.0.1` n'isole de rien : toute application de la machine
> peut y émettre des requêtes, **y compris une page web ouverte dans un
> navigateur**. Sans jeton, un site visité pourrait lire ta bibliothèque ou y
> injecter des fichiers.

---

### `GET /api/v1/health`

À appeler avant tout : Onzer tourne-t-il, et le SSD est-il branché ?

```json
{
  "status": "ok",
  "version": "0.1.0",
  "libraryOnline": true,
  "inbox": "/Volumes/Lexar/Musique/_Inbox",
  "trackCount": 1247
}
```

Si `libraryOnline` est `false`, **n'importe rien** : le volume n'est pas monté.

---

### `GET /api/v1/exists`

**La route qui fait gagner du temps.** À interroger *avant* de télécharger.

| Paramètre | Requis | Rôle |
|---|---|---|
| `title` | ✅ | Titre du morceau |
| `artist` | — | Améliore beaucoup la fiabilité |
| `durationMs` | — | Départage deux morceaux homonymes |

```
GET /api/v1/exists?title=Digital%20Love&artist=Daft%20Punk&durationMs=301000
```

```json
{ "exists": true, "trackId": 42 }
```

La comparaison est insensible à la casse, aux accents et à la ponctuation :
`A$AP Rocky` et `ASAP Rocky` sont reconnus comme le même artiste. Sans
`durationMs`, la réponse est plus permissive — donc plus sujette aux faux
positifs sur des titres génériques.

---

### `POST /api/v1/import`

```json
{
  "path": "/Users/carlos/Downloads/telechargement.mp3",
  "metadata": {
    "title": "Digital Love",
    "artist": "Daft Punk",
    "album": "Discovery",
    "albumArtist": "Daft Punk",
    "year": 2001,
    "trackNo": 3,
    "genre": "French House"
  }
}
```

`path` doit être **absolu** et pointer un fichier déjà téléchargé. On transmet
un chemin plutôt que le contenu : le fichier est déjà sur le disque, l'envoyer
en HTTP ne ferait que le copier deux fois.

Le bloc `metadata` est entièrement optionnel.

**Réponse — import réussi :**

```json
{
  "outcome": "imported",
  "trackId": 1248,
  "relativePath": "Daft Punk/2001 - Discovery/03 - Digital Love.mp3",
  "reason": null
}
```

**Réponse — doublon :**

```json
{
  "outcome": "duplicate",
  "trackId": 42,
  "relativePath": null,
  "reason": "fichier identique"
}
```

En cas de doublon, **le fichier source n'est pas touché.**

---

## Arbitrage des métadonnées

C'est le point le plus subtil du contrat, et il évite les mauvaises surprises.

| Situation | Qui gagne |
|---|---|
| Le fichier a de vrais tags | **Les tags du fichier** |
| Le fichier n'a aucun tag, Onzer a deviné depuis le nom de fichier | **Tes indications** |
| Le fichier n'a pas ce champ | **Tes indications** |
| Champs numériques (`year`, `trackNo`, `discNo`) | Les tags du fichier, toujours |

**Le raisonnement :** un vrai tag est une information, un nom de fichier est une
supposition. Ton script connaît la page source ; il bat donc une supposition,
mais pas une donnée renseignée par la personne qui a encodé le fichier.

---

## Codes d'erreur

| Code | Signification |
|---|---|
| `401` | Jeton absent ou invalide |
| `400` | Chemin non absolu, ou titre manquant sur `/exists` |
| `404` | Fichier introuvable |
| `415` | Extension audio non reconnue |
| `503` | Bibliothèque hors ligne — le SSD n'est pas monté |
| `500` | Erreur interne, message détaillé dans le corps |

Toutes les erreurs renvoient le même format :

```json
{ "error": "description lisible" }
```

---

## Exemple complet

Un client Python sans dépendance est fourni :
[`tools/downloader/onzer_client.py`](../tools/downloader/onzer_client.py).

```bash
python3 tools/downloader/onzer_client.py health
python3 tools/downloader/onzer_client.py exists "Digital Love" --artist "Daft Punk"
python3 tools/downloader/onzer_client.py import ~/Downloads/x.mp3 --artist "Daft Punk" --title "Digital Love"
```

---

## Formats acceptés

`mp3` · `flac` · `m4a` · `aac` · `ogg` · `opus` · `wav` · `aiff` · `wv`

## Stabilité du contrat

Le préfixe `/api/v1` fait foi. Toute évolution incompatible passera par
`/api/v2`, sans casser les scripts existants.
