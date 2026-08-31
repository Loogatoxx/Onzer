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
