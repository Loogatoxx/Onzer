//! Politique de limitation et de réessai.
//!
//! # Pourquoi c'est la première brique écrite
//!
//! AcoustID et MusicBrainz sont des services **gratuits, bénévoles et
//! rationnés**. MusicBrainz impose une requête par seconde et bannit les
//! adresses qui l'ignorent. Un client naïf qui lancerait 5 000 requêtes en
//! rafale se ferait couper l'accès en quelques secondes — définitivement pour
//! certaines adresses.
//!
//! Ce module isole toute la politique dans de la **logique pure**, sans réseau
//! ni horloge réelle. Elle est donc entièrement testable, ce qui compte
//! d'autant plus qu'un défaut ici ne se manifesterait qu'une fois le mal fait.
//!
//! # Les quatre règles
//!
//! | Situation | Décision |
//! |---|---|
//! | `429 Too Many Requests` | Attendre **exactement** ce que dit `Retry-After`, sinon repli exponentiel |
//! | Erreur serveur (5xx) | Repli exponentiel : c'est passager |
//! | Erreur client (4xx) | **Abandonner** : notre requête est fautive, la répéter n'y changera rien |
//! | Panne réseau | Repli exponentiel |

use std::time::Duration;

/// Intervalle minimal exigé par MusicBrainz. Une requête par seconde, et une
/// marge de 100 ms pour absorber les imprécisions d'horloge.
pub const MUSICBRAINZ_MIN_INTERVAL: Duration = Duration::from_millis(1_100);

/// AcoustID tolère trois requêtes par seconde. On reste volontairement en
/// dessous : rien ne presse, l'identification se fait en tâche de fond.
pub const ACOUSTID_MIN_INTERVAL: Duration = Duration::from_millis(500);

/// Paramètres du repli exponentiel.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    pub base: Duration,
    pub max: Duration,
    /// Nombre total de tentatives, première comprise.
    pub max_attempts: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(2),
            // Au-delà d'une minute d'attente, insister n'a plus de sens dans un
            // travail de fond : on reprendra au prochain passage de l'ouvrier.
            max: Duration::from_secs(60),
            max_attempts: 4,
        }
    }
}

impl Backoff {
    /// Délai avant la tentative numéro `attempt` (la première vaut 1).
    ///
    /// Croissance exponentielle **assortie d'une gigue** : sans elle, plusieurs
    /// clients ayant échoué au même instant réessaieraient tous exactement en
    /// même temps, reproduisant la surcharge qu'ils viennent de causer.
    pub fn delay(&self, attempt: u32, jitter_seed: u64) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        let raw = self.base.saturating_mul(1_u32 << exponent);
        let capped = raw.min(self.max);

        // Gigue de ±25 %, déterministe pour rester testable.
        let jitter_ratio = 0.75 + (jitter_seed % 500) as f64 / 1000.0;
        capped.mul_f64(jitter_ratio)
    }
}

/// Ce qu'il faut faire après une réponse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Réessayer après ce délai.
    Retry(Duration),
    /// Renoncer : soit la faute vient de nous, soit on a assez insisté.
    GiveUp,
}

/// Issue d'une tentative, telle que le client la constate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Réponse HTTP reçue, avec son code.
    Status(u16),
    /// Aucune réponse : coupure réseau, DNS, délai dépassé.
    NetworkFailure,
}

/// Décide de la suite après une tentative échouée.
///
/// `retry_after` provient de l'en-tête du même nom. Quand un service dit
/// explicitement combien de temps attendre, **c'est cette valeur qui fait
/// autorité** : y substituer notre propre calcul serait le meilleur moyen de se
/// faire bannir.
pub fn decide(
    outcome: Outcome,
    attempt: u32,
    retry_after: Option<Duration>,
    backoff: &Backoff,
    jitter_seed: u64,
) -> Decision {
    if attempt >= backoff.max_attempts {
        return Decision::GiveUp;
    }

    match outcome {
        Outcome::Status(429) => {
            // Le service a dit son mot ; on l'écoute, en plafonnant tout de
            // même pour ne pas bloquer l'ouvrier une demi-heure.
            let wait = retry_after
                .unwrap_or_else(|| backoff.delay(attempt, jitter_seed))
                .min(backoff.max);
            Decision::Retry(wait)
        }

        // 5xx : le service a un problème passager, pas nous.
        Outcome::Status(status) if (500..600).contains(&status) => {
            Decision::Retry(backoff.delay(attempt, jitter_seed))
        }

        // 4xx : notre requête est fautive — clé invalide, empreinte malformée.
        // La répéter ne ferait que consommer du quota pour rien.
        Outcome::Status(_) => Decision::GiveUp,

        Outcome::NetworkFailure => Decision::Retry(backoff.delay(attempt, jitter_seed)),
    }
}

/// Limiteur à intervalle minimal, partagé entre toutes les requêtes d'un même
/// service.
///
/// Sérialise les appels : une requête ne part jamais avant que l'intervalle
/// exigé se soit écoulé depuis la précédente.
pub struct RateLimiter {
    min_interval: Duration,
    last_call: tokio::sync::Mutex<Option<tokio::time::Instant>>,
}

impl RateLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_call: tokio::sync::Mutex::new(None),
        }
    }

    /// Attend le temps nécessaire, puis rend la main.
    ///
    /// Le verrou est **tenu pendant l'attente** : c'est ce qui garantit que
    /// deux appels concurrents ne partent pas ensemble après avoir tous deux
    /// constaté que l'intervalle était écoulé.
    pub async fn acquire(&self) {
        let mut last = self.last_call.lock().await;

        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval - elapsed).await;
            }
        }

        *last = Some(tokio::time::Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backoff() -> Backoff {
        Backoff::default()
    }

    // ── Repli exponentiel ───────────────────────────────────────────────

    #[test]
    fn le_delai_croit_de_facon_exponentielle() {
        let backoff = backoff();

        let premier = backoff.delay(1, 0);
        let deuxieme = backoff.delay(2, 0);
        let troisieme = backoff.delay(3, 0);

        assert!(deuxieme > premier);
        assert!(troisieme > deuxieme);
        assert!(troisieme >= premier * 3, "la croissance doit être franche");
    }

    #[test]
    fn le_delai_est_plafonne() {
        let backoff = backoff();

        // Une tentative très lointaine ne doit pas produire une attente absurde.
        assert!(backoff.delay(30, 0) <= backoff.max);
    }

    #[test]
    fn la_gigue_desynchronise_les_clients() {
        let backoff = backoff();

        // Sans gigue, tous les clients ayant échoué ensemble réessaieraient
        // exactement en même temps, reproduisant la surcharge.
        let delais: Vec<Duration> = (0..50).map(|seed| backoff.delay(3, seed)).collect();
        let distincts: std::collections::HashSet<_> = delais.iter().collect();

        assert!(distincts.len() > 10, "gigue insuffisante : {distincts:?}");
    }

    #[test]
    fn la_gigue_reste_dans_une_plage_raisonnable() {
        let backoff = backoff();
        let reference = backoff.base * 4; // tentative 3, sans gigue

        for seed in 0..500 {
            let delai = backoff.delay(3, seed);
            assert!(
                delai >= reference.mul_f64(0.7) && delai <= reference.mul_f64(1.3),
                "graine {seed} : délai {delai:?} hors de ±30 %"
            );
        }
    }

    // ── Décisions ───────────────────────────────────────────────────────

    #[test]
    fn une_erreur_client_fait_abandonner_immediatement() {
        // Clé d'API invalide, empreinte malformée : réessayer ne ferait que
        // consommer du quota pour rien.
        for status in [400, 401, 403, 404] {
            assert_eq!(
                decide(Outcome::Status(status), 1, None, &backoff(), 0),
                Decision::GiveUp,
                "code {status}"
            );
        }
    }

    #[test]
    fn une_erreur_serveur_est_reessayee() {
        for status in [500, 502, 503, 504] {
            assert!(
                matches!(
                    decide(Outcome::Status(status), 1, None, &backoff(), 0),
                    Decision::Retry(_)
                ),
                "code {status}"
            );
        }
    }

    #[test]
    fn une_panne_reseau_est_reessayee() {
        assert!(matches!(
            decide(Outcome::NetworkFailure, 1, None, &backoff(), 0),
            Decision::Retry(_)
        ));
    }

    #[test]
    fn le_service_a_toujours_raison_sur_le_delai_dattente() {
        // C'est LA règle qui évite le bannissement : quand un service dit
        // combien de temps attendre, on l'écoute au lieu de calculer.
        let impose = Duration::from_secs(17);

        assert_eq!(
            decide(Outcome::Status(429), 1, Some(impose), &backoff(), 0),
            Decision::Retry(impose)
        );
    }

    #[test]
    fn un_delai_impose_absurde_reste_plafonne() {
        // Un service qui répondrait « reviens dans deux heures » bloquerait
        // l'ouvrier : on plafonne et on reprendra au passage suivant.
        let decision = decide(
            Outcome::Status(429),
            1,
            Some(Duration::from_secs(7200)),
            &backoff(),
            0,
        );

        assert_eq!(decision, Decision::Retry(backoff().max));
    }

    #[test]
    fn un_429_sans_indication_retombe_sur_le_repli() {
        assert!(matches!(
            decide(Outcome::Status(429), 1, None, &backoff(), 0),
            Decision::Retry(_)
        ));
    }

    #[test]
    fn on_finit_par_renoncer() {
        let backoff = backoff();

        assert_eq!(
            decide(
                Outcome::NetworkFailure,
                backoff.max_attempts,
                None,
                &backoff,
                0
            ),
            Decision::GiveUp
        );
    }

    #[test]
    fn le_nombre_de_tentatives_est_respecte() {
        let backoff = backoff();
        let mut tentatives = 0;

        for attempt in 1..=10 {
            match decide(Outcome::Status(503), attempt, None, &backoff, 0) {
                Decision::Retry(_) => tentatives += 1,
                Decision::GiveUp => break,
            }
        }

        assert_eq!(tentatives, backoff.max_attempts - 1);
    }

    // ── Limiteur ────────────────────────────────────────────────────────

    #[tokio::test(start_paused = true)]
    async fn le_limiteur_espace_les_appels() {
        let limiter = RateLimiter::new(Duration::from_millis(1_000));
        let debut = tokio::time::Instant::now();

        limiter.acquire().await; // immédiat
        limiter.acquire().await; // doit attendre 1 s
        limiter.acquire().await; // encore 1 s

        assert!(
            debut.elapsed() >= Duration::from_millis(2_000),
            "trois appels espacés d'une seconde prennent au moins deux secondes"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn le_premier_appel_ne_fait_pas_attendre() {
        let limiter = RateLimiter::new(Duration::from_secs(5));
        let debut = tokio::time::Instant::now();

        limiter.acquire().await;

        assert!(debut.elapsed() < Duration::from_millis(50));
    }

    /// Vérifie que les cadences déclarées ordonnent bien les deux services.
    ///
    /// MusicBrainz est le plus strict (une requête par seconde) ; AcoustID en
    /// tolère trois. Inverser les deux constantes par inadvertance ferait
    /// bannir sur le service le plus sévère.
    #[test]
    fn musicbrainz_est_plus_menage_quacoustid() {
        assert!(MUSICBRAINZ_MIN_INTERVAL > ACOUSTID_MIN_INTERVAL);

        let cadences = [MUSICBRAINZ_MIN_INTERVAL, ACOUSTID_MIN_INTERVAL];
        assert!(cadences.iter().all(|interval| *interval >= Duration::from_millis(334)));
    }
}
