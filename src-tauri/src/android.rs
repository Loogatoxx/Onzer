//! Le pont vers la machine virtuelle Java.
//!
//! # Le mur, et sa cause exacte
//!
//! Sur Android, `cpal` — la couche audio de `rodio` — demande le **contexte
//! Android** pour ouvrir un flux : la machine virtuelle Java et l'objet
//! `Context` de l'application. Il ne les cherche pas lui-même, il les lit dans
//! `ndk_context`, un dépôt global qu'un autre est censé avoir rempli.
//!
//! Personne ne le remplissait. `cpal` ne rendait pas une erreur pour autant :
//! il **paniquait**, le fil audio mourait sans un mot, et l'application ne
//! voyait qu'un canal fermé. Le message d'origine, retrouvé en rattrapant la
//! panique, tenait en cinq mots : « android context was not initialized ».
//!
//! # Pourquoi `JNI_OnLoad` et non un appel depuis Kotlin
//!
//! On pourrait ajouter une méthode native à `MainActivity` et l'appeler à son
//! démarrage. Ce serait fragile : il faudrait garantir que cet appel précède
//! la création du moteur audio, alors que les deux sont déclenchés par Tauri.
//!
//! `JNI_OnLoad` est appelé par la machine virtuelle **au chargement de la
//! bibliothèque native**, avant que la moindre ligne de notre code ne tourne.
//! L'ordre n'est plus une question de discipline, c'est une garantie.
//!
//! # Pourquoi `ActivityThread` plutôt que l'activité
//!
//! Au chargement de la bibliothèque, aucune activité n'existe encore. Mais
//! Android tient un accès statique à l'application elle-même :
//! `ActivityThread.currentApplication()`. C'est un `Context` parfaitement
//! valable pour ouvrir un flux audio, et il vit aussi longtemps que le
//! processus.

use std::ffi::c_void;

use jni::objects::JObject;
use jni::sys::{jint, JavaVM as RawJavaVM, JNI_VERSION_1_6};
use jni::{JNIEnv, JavaVM};

/// Appelé par la machine virtuelle quand `libonzer_lib.so` est chargée.
///
/// # Sûreté
///
/// Le pointeur vient de la machine virtuelle elle-même ; il est valide pour
/// toute la durée du processus.
#[no_mangle]
pub unsafe extern "C" fn JNI_OnLoad(vm: *mut RawJavaVM, _reserved: *mut c_void) -> jint {
    if let Err(error) = installer_le_contexte(vm) {
        // Rien à faire de plus : sans contexte, la lecture sera indisponible
        // et l'interface le dira. Mais la bibliothèque doit se charger, sans
        // quoi c'est l'application entière qui ne démarre pas.
        tracing::error!(%error, "contexte Android non installé");
    }

    JNI_VERSION_1_6
}

unsafe fn installer_le_contexte(raw: *mut RawJavaVM) -> Result<(), String> {
    let vm = JavaVM::from_raw(raw).map_err(|error| error.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|error| error.to_string())?;

    // # Pourquoi la classe est capturée maintenant
    //
    // `FindClass` ne trouve les classes de l'application que si le fil courant
    // porte son chargeur de classes. C'est le cas ici — la machine virtuelle
    // vient de charger notre bibliothèque — et **ce n'est plus le cas** sur un
    // fil attaché depuis Rust : celui-là hérite du chargeur système, qui ne
    // connaît que `java.*` et `android.*`.
    //
    // L'erreur est spectaculairement trompeuse : « ClassNotFoundException »
    // pour une classe pourtant bien présente dans le `.dex`.
    if let Ok(classe) = env.find_class("com/loogatoxx/onzer/PlaybackService") {
        if let Ok(globale) = env.new_global_ref(&classe) {
            let _ = SERVICE.set(globale);
        }
    }

    let application = application_courante(&mut env)?;

    // La référence globale doit survivre à cette fonction : `ndk_context` en
    // garde le pointeur brut pour toute la vie du processus.
    let global = env
        .new_global_ref(&application)
        .map_err(|error| error.to_string())?;

    let contexte = global.as_raw().cast::<c_void>();
    ndk_context::initialize_android_context(raw.cast::<c_void>(), contexte);

    installer_le_verificateur_tls(&mut env, global.as_raw());

    std::mem::forget(global);
    Ok(())
}

/// Donne à `rustls` de quoi vérifier un certificat.
///
/// # Le défaut que ça corrige
///
/// Sur Android, `reqwest` en `rustls` s'en remet au magasin de confiance du
/// système, via `rustls-platform-verifier`. Ce vérificateur exige d'être
/// initialisé avec le contexte de l'application — et quand il ne l'est pas, il
/// ne rend pas une erreur : il appelle `abort()`.
///
/// L'application disparaît donc **à la première connexion HTTPS**, sans rien
/// afficher, sans rien écrire dans un journal lisible. Le message n'existe que
/// dans le tombstone du système :
///
/// ```text
/// signal 6 (SIGABRT) · Abort message: 'Expect rustls-platform-verifier to be initialized'
/// ```
///
/// Toute la moitié en ligne d'Onzer passait par là : les paroles, les
/// pochettes, les albums, la comparaison d'une playlist. Rien de tout cela ne
/// pouvait fonctionner sur le téléphone, et l'écran ne montrait qu'une
/// application qui se ferme.
///
/// # Pourquoi les pointeurs bruts
///
/// Ce crate parle `jni` 0.22, notre pont parle 0.21. Les deux versions
/// cohabitent dans le binaire sans se connaître, et aucun de leurs types n'est
/// convertible en l'autre. Ce qu'elles partagent, c'est ce que la machine
/// virtuelle leur a donné : deux pointeurs, identiques des deux côtés.
///
/// # Pourquoi un échec n'est pas fatal
///
/// Sans vérificateur, la partie hors ligne — c'est-à-dire l'essentiel —
/// fonctionne. La perdre pour un magasin de certificats serait absurde.
unsafe fn installer_le_verificateur_tls(env: &mut JNIEnv<'_>, contexte: jni::sys::jobject) {
    let brut = env.get_raw();
    let mut emprunte = jni22::EnvUnowned::from_raw(brut.cast());

    // Le résultat sort par une variable et non par la valeur de retour : cette
    // dernière est destinée à être rendue à Java, ce qui n'a pas de sens ici.
    let mut resultat = Ok(());

    let _ = emprunte.with_env_no_catch(|env22| {
        let contexte22 = jni22::objects::JObject::from_raw(env22, contexte.cast());
        resultat = rustls_platform_verifier::android::init_with_env(env22, contexte22);
        Ok::<(), jni22::errors::Error>(())
    });

    match resultat {
        Ok(()) => tracing::info!("vérificateur TLS installé"),
        Err(erreur) => tracing::error!(
            %erreur,
            "vérificateur TLS non installé : les services en ligne seront indisponibles"
        ),
    }
}

/// L'objet `Application`, obtenu sans passer par une activité.
unsafe fn application_courante<'a>(env: &mut JNIEnv<'a>) -> Result<JObject<'a>, String> {
    let classe = env
        .find_class("android/app/ActivityThread")
        .map_err(|error| format!("ActivityThread introuvable : {error}"))?;

    let application = env
        .call_static_method(
            classe,
            "currentApplication",
            "()Landroid/app/Application;",
            &[],
        )
        .map_err(|error| format!("currentApplication a échoué : {error}"))?
        .l()
        .map_err(|error| error.to_string())?;

    if application.is_null() {
        return Err("aucune application courante".to_string());
    }

    Ok(application)
}

// ════════════════════════════════════════════════════════════════════════════
//  La lecture, vue par le système
// ════════════════════════════════════════════════════════════════════════════

/// La poignée de l'application, pour que le service puisse commander.
///
/// Les fonctions appelées depuis Kotlin n'ont aucun contexte : elles arrivent
/// sur un fil de la machine virtuelle, sans rien de Tauri autour. Cette
/// référence est le seul moyen de retrouver le lecteur.
static POIGNEE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// La classe du service, capturée au chargement de la bibliothèque.
///
/// Voir `installer_le_contexte` : c'est le seul moment où le chargeur de
/// classes de l'application est en vue.
static SERVICE: std::sync::OnceLock<jni::objects::GlobalRef> = std::sync::OnceLock::new();

/// À appeler une fois au démarrage.
pub fn retenir_la_poignee(app: tauri::AppHandle) {
    let _ = POIGNEE.set(app);
}

/// Pousse l'état de lecture vers le service Android.
///
/// # Pourquoi la pochette en base64
///
/// Franchir la frontière JNI avec un tableau d'octets demande un
/// `JByteArray`, sa copie et sa libération. Une chaîne fait le même travail
/// pour quelques kilo-octets — et la vignette qu'on envoie en fait moins de
/// trente. Le coût est invisible, le code est trois fois plus court.
pub fn pousser_letat(
    titre: &str,
    artiste: &str,
    en_lecture: bool,
    position_ms: i64,
    duree_ms: i64,
    pochette_base64: &str,
) {
    if let Err(error) = pousser(titre, artiste, en_lecture, position_ms, duree_ms, pochette_base64)
    {
        // Le son continue sans la notification : c'est dégradé, pas cassé.
        tracing::warn!(%error, "état de lecture non transmis au système");
    }
}

fn pousser(
    titre: &str,
    artiste: &str,
    en_lecture: bool,
    position_ms: i64,
    duree_ms: i64,
    pochette_base64: &str,
) -> Result<(), String> {
    let contexte = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(contexte.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;

    let activite = unsafe { JObject::from_raw(contexte.context().cast()) };

    let classe = SERVICE
        .get()
        .ok_or_else(|| "classe du service non capturée au chargement".to_string())?;

    let titre = env.new_string(titre).map_err(|e| e.to_string())?;
    let artiste = env.new_string(artiste).map_err(|e| e.to_string())?;
    let pochette = env.new_string(pochette_base64).map_err(|e| e.to_string())?;

    // La référence globale porte un `JObject` : il faut le présenter comme la
    // classe qu'il est.
    let classe = jni::objects::JClass::from(unsafe {
        JObject::from_raw(classe.as_obj().as_raw())
    });

    env.call_static_method(
        &classe,
        "pousser",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;ZJJLjava/lang/String;)V",
        &[
            (&activite).into(),
            (&titre).into(),
            (&artiste).into(),
            jni::objects::JValue::Bool(u8::from(en_lecture)),
            jni::objects::JValue::Long(position_ms),
            jni::objects::JValue::Long(duree_ms),
            (&pochette).into(),
        ],
    )
    .map_err(|e| format!("appel de pousser : {e}"))?;

    // Une exception Java laissée en suspens fait planter le prochain appel
    // JNI, où qu'il ait lieu.
    let _ = env.exception_clear();
    Ok(())
}

/// Exécute une commande de lecture venue du système, puis publie l'état.
///
/// # Pourquoi tout passe par ici
///
/// Les quatre commandes ne diffèrent que par une ligne. Ce qui les entoure —
/// retrouver la poignée, l'état, le lecteur, republier l'instantané — est
/// identique, et c'est précisément la partie qu'on oublie : sans le dernier
/// `emit`, l'interface continuerait d'afficher « en pause » alors que la
/// notification vient de relancer la musique.
fn commander<F>(action: F)
where
    F: for<'a> FnOnce(
            &'a crate::audio::PlayerService,
            &'a sqlx::SqlitePool,
            &'a crate::core::PathResolver,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>
        + Send
        + 'static,
{
    let Some(app) = POIGNEE.get().cloned() else {
        return;
    };

    tauri::async_runtime::spawn(async move {
        let state = tauri::Manager::state::<crate::AppState>(&app);
        let Ok(player) = state.player() else {
            return;
        };

        let paths = state.paths.read().await.clone();
        action(player, &state.pool, &paths).await;

        let _ = tauri::Emitter::emit(
            &app,
            crate::commands::playback::STATE_EVENT,
            player.snapshot().await,
        );
    });
}

#[no_mangle]
pub extern "system" fn Java_com_loogatoxx_onzer_PlaybackService_natifBasculer(
    _env: JNIEnv,
    _classe: JObject,
) {
    commander(|player, _pool, _paths| Box::pin(async move { let _ = player.toggle().await; }));
}

#[no_mangle]
pub extern "system" fn Java_com_loogatoxx_onzer_PlaybackService_natifSuivant(
    _env: JNIEnv,
    _classe: JObject,
) {
    commander(|player, pool, paths| {
        Box::pin(async move {
            let _ = player.next(pool, paths, false).await;
        })
    });
}

#[no_mangle]
pub extern "system" fn Java_com_loogatoxx_onzer_PlaybackService_natifPrecedent(
    _env: JNIEnv,
    _classe: JObject,
) {
    commander(|player, pool, paths| {
        Box::pin(async move {
            let _ = player.previous(pool, paths).await;
        })
    });
}

#[no_mangle]
pub extern "system" fn Java_com_loogatoxx_onzer_PlaybackService_natifPositionner(
    _env: JNIEnv,
    _classe: JObject,
    position_ms: i64,
) {
    commander(move |player, _pool, _paths| {
        Box::pin(async move {
            let _ = player.seek(position_ms).await;
        })
    });
}

#[no_mangle]
pub extern "system" fn Java_com_loogatoxx_onzer_PlaybackService_natifArreter(
    _env: JNIEnv,
    _classe: JObject,
) {
    commander(|player, pool, _paths| {
        Box::pin(async move {
            let _ = player.stop(pool).await;
        })
    });
}
