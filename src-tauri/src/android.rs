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

    let application = application_courante(&mut env)?;

    // La référence globale doit survivre à cette fonction : `ndk_context` en
    // garde le pointeur brut pour toute la vie du processus.
    let global = env
        .new_global_ref(&application)
        .map_err(|error| error.to_string())?;

    let contexte = global.as_raw().cast::<c_void>();
    ndk_context::initialize_android_context(raw.cast::<c_void>(), contexte);

    std::mem::forget(global);
    Ok(())
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
