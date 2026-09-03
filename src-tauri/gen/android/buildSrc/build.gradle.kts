plugins {
    `kotlin-dsl`
}

gradlePlugin {
    plugins {
        create("pluginsForCoolKids") {
            id = "rust"
            implementationClass = "RustPlugin"
        }
    }
}

repositories {
    google()
    mavenCentral()
}

dependencies {
    compileOnly(gradleApi())
    implementation("com.android.tools.build:gradle:8.11.0")
}


// Même raison que le projet principal : exFAT et Gradle ne s'entendent pas.
// `buildSrc` est une compilation à part, avec son propre dossier de sortie.
layout.buildDirectory.set(file("/Volumes/OnzerBuild/android/buildSrc"))
