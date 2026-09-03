package com.loogatoxx.onzer

import android.Manifest
import android.content.Intent
import android.graphics.Color
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

/**
 * Point d'entrée Android.
 *
 * # Deux autorisations, deux natures
 *
 * `READ_MEDIA_AUDIO` s'accorde d'un bouton et suffit à **lister** les
 * fichiers. Elle ne suffit pas à les ouvrir ni à les déplacer : le stockage
 * cantonné réserve cela à MediaStore. Or Onzer range les morceaux par artiste,
 * année et album — déplacer des fichiers est le cœur de ce qu'il fait. Sans
 * plus, les 2699 fichiers trouvés rendaient tous « Operation not permitted ».
 *
 * `MANAGE_EXTERNAL_STORAGE` lève cette limite. Elle ne s'accorde pas d'un
 * bouton : le système exige que l'utilisateur ouvre ses réglages et l'active
 * lui-même. On l'y emmène directement plutôt que de le laisser chercher.
 */
class MainActivity : TauriActivity() {
  companion object {
    /** `--color-base` de la feuille de style : #08080a. */
    private val FOND = Color.rgb(8, 8, 10)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    // # Pourquoi la barre d'état est peinte à la main
    //
    // `enableEdgeToEdge()` étend bien la page sous les barres système, mais
    // les laisse sur leur fond par défaut — un noir qui n'est pas le nôtre.
    // Deux noirs voisins se voient : la barre se détache au lieu de
    // prolonger l'application. On lui donne donc exactement `--color-base`.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(FOND),
      navigationBarStyle = SystemBarStyle.dark(FOND),
    )
    super.onCreate(savedInstanceState)
    demanderLAccesALaMusique()
    demanderLAccesComplet()
  }

  private fun demanderLAccesALaMusique() {
    // Android 13 a scindé l'ancienne permission de stockage par type de
    // média. Demander la mauvaise sur la mauvaise version ne fait rien.
    val permission =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        Manifest.permission.READ_MEDIA_AUDIO
      } else {
        Manifest.permission.READ_EXTERNAL_STORAGE
      }

    val accordee =
      ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

    if (!accordee) {
      ActivityCompat.requestPermissions(this, arrayOf(permission), 1)
    }
  }

  private fun demanderLAccesComplet() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
    if (Environment.isExternalStorageManager()) return

    // On vise directement la page de l'application : la page générale oblige
    // à retrouver Onzer dans une liste de deux cents entrées.
    val intention =
      Intent(
        Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
        Uri.parse("package:$packageName"),
      )

    runCatching { startActivity(intention) }
      .onFailure { startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)) }
  }
}
