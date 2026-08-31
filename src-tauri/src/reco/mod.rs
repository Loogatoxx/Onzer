//! Moteur de recommandation local.
//!
//! # Ce qu'on ne peut pas faire, et pourquoi
//!
//! Spotify recommande par **filtrage collaboratif** : « les gens qui aiment X
//! aiment aussi Y », calculé sur des centaines de millions d'utilisateurs.
//! Onzer a un seul utilisateur et un catalogue fermé. Reproduire cette approche
//! est structurellement impossible, et prétendre le contraire produirait un
//! moteur vide.
//!
//! # Ce qu'on peut faire mieux
//!
//! Spotify excelle à faire **découvrir des artistes inconnus**. Il est en
//! revanche médiocre pour choisir *le bon morceau de ta propre bibliothèque,
//! maintenant*. C'est sur ce terrain qu'Onzer joue, avec trois avantages réels :
//!
//! | Avantage | Détail |
//! |---|---|
//! | Aucun biais commercial | Rien n'est poussé pour des raisons contractuelles |
//! | Signaux bien plus riches | Position exacte du saut, appareil de sortie, contexte horaire |
//! | Catalogue assez petit | Similarité exhaustive sur toute la bibliothèque, impossible à leur échelle |
//!
//! # Les quatre fondations
//!
//! | Module | Question à laquelle il répond | Fonctionne sans historique |
//! |---|---|---|
//! | [`crate::analysis`] | À quoi ressemble ce morceau ? | ✅ |
//! | [`similarity`] | Quels morceaux se ressemblent ? | ✅ |
//! | [`affinity`] | Combien l'aimes-tu, aujourd'hui ? | ❌ |
//! | [`bandit`] | Quelle stratégie marche chez toi ? | ❌ mais converge vite |
//!
//! Les deux premières portent le démarrage à froid : dès qu'un morceau est
//! analysé, il est recommandable. Les deux dernières prennent le relais à
//! mesure que l'historique s'étoffe.
//!
//! # Le principe qui gouverne le reste
//!
//! > Sans mesure, une recommandation n'est que de l'astrologie.
//!
//! Chaque proposition est tracée avec la stratégie qui l'a produite, et son
//! issue est enregistrée. Les vues `reco_quality` et `reco_strategy_quality`
//! comparent le taux de complétion des morceaux proposés par le moteur à celui
//! des morceaux choisis à la main ou tirés au hasard. Si le moteur fait moins
//! bien que le hasard, on doit pouvoir le lire noir sur blanc.

pub mod affinity;
pub mod bandit;
pub mod similarity;

pub use bandit::{Arm, Rng, Strategy};
pub use similarity::FeatureSpace;
