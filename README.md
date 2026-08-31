# 🎧 Onzer

**Un lecteur musical 100 % local, hors-ligne et souverain.**
Ta musique, sur ton disque, avec un moteur de recommandation qui apprend de toi — sans cloud, sans compte, sans télémétrie.

---

## Le concept

Onzer est un « Spotify local » : tu apportes tes propres fichiers audio, l'application les range,
les analyse et apprend tes habitudes d'écoute pour te proposer la bonne musique au bon moment.

### Les 4 piliers

| Pilier | Description |
|---|---|
| 🎵 **Lecteur offline** | Lecture gapless, crossfade, normalisation ReplayGain. Zéro connexion requise. |
| 🧠 **Reco IA locale** | Analyse du signal audio + apprentissage de tes écoutes, skips, contextes horaires. |
| 📊 **Stats poussées** | Dashboard permanent + « Onzer Wrapped » généré entièrement en local. |
| 🤖 **Import automatique** | Un dossier surveillé et une API locale : un script externe dépose, Onzer range et tague. |

---

## Stack technique

- **Shell** : [Tauri 2](https://v2.tauri.app/)
- **Backend** : Rust (audio, base de données, analyse, recommandation)
- **Frontend** : React 19 + TypeScript + Vite
- **UI** : Tailwind CSS v4 + shadcn/ui + Motion
- **Base de données** : SQLite (WAL + FTS5)

Cible actuelle : **macOS**. L'architecture est volontairement *Android-ready* — voir
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

---

## Prérequis

| Outil | Version | Statut |
|---|---|---|
| Node.js | ≥ 20 | ✅ |
| Xcode Command Line Tools | — | ✅ |
| Rust | ≥ 1.77 | ⚠️ à installer |

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## Développement

```bash
npm install
npm run tauri dev
```

---

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — décisions d'architecture (ADR)
- [`historique_prompts.md`](historique_prompts.md) — journal de développement

---

## Licence

Projet personnel. Onzer ne télécharge, ne distribue et n'héberge aucun contenu :
il lit uniquement les fichiers que l'utilisateur possède déjà sur sa machine.
