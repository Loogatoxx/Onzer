buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.25")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

tasks.register("clean").configure {
    delete("build")
}


// ── Pourquoi la compilation sort du SSD ──────────────────────────────────
//
// Le projet vit sur un volume exFAT, seul format que macOS et Android
// partagent. exFAT ne connaît pas les attributs étendus : macOS les stocke
// donc dans des fichiers jumeaux « ._quelquechose », invisibles au Finder et
// bien réels pour tout le reste.
//
// Gradle s'y casse les dents de deux façons : son analyse d'instrumentation
// les lit comme s'ils étaient des classes, et son nettoyage échoue à les
// supprimer — « Unable to delete directory … ._shrunk-classpath-snapshot.bin ».
//
// Les sorties de compilation partent donc sur le volume APFS déjà utilisé
// pour le cache Rust : même raisonnement, même endroit. Rien de ce qui s'y
// écrit n'est une source, tout s'y reconstruit.
// Seul `buildSrc` déménage — voir son propre fichier. Les modules du projet
// restent où Tauri les attend : c'est lui qui va chercher l'APK produit, et
// déplacer sa sortie revient à la lui cacher.
