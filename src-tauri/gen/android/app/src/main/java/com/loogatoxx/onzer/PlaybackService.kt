package com.loogatoxx.onzer

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.IBinder
import android.util.Base64

/**
 * La lecture, vue par le système.
 *
 * # Ce que ce service apporte, et pourquoi il faut un service
 *
 * Le son est produit par le cœur Rust, qui n'a besoin de personne pour
 * fonctionner. Mais Android ne le sait pas : dès que l'écran s'éteint, il
 * considère l'application comme inactive et se réserve le droit de la tuer.
 *
 * Un **service de premier plan** est la seule façon de lui dire « ceci
 * continue » — et la contrepartie exigée est une notification permanente. Ce
 * n'est pas un ennui, c'est la contrepartie : elle porte les commandes.
 *
 * # Pourquoi une MediaSession et pas seulement une notification
 *
 * La session est ce que le système interroge pour peupler l'écran verrouillé,
 * les boutons des écouteurs Bluetooth, la tuile du volet des réglages et les
 * montres connectées. Une notification seule n'alimenterait que la notification.
 *
 * # Qui commande qui
 *
 * ```text
 *   Rust (le son)  ──► pousser(titre, artiste, en lecture…)  ──► MediaSession
 *                                                                    │
 *   Rust (le son)  ◄──  natifBasculer() / natifSuivant()  ◄──────────┘
 * ```
 *
 * Rust reste la source de vérité : le service n'a aucun état à lui, il
 * reflète et transmet.
 */
class PlaybackService : Service() {
  private lateinit var session: MediaSession

  override fun onCreate() {
    super.onCreate()

    session = MediaSession(this, "Onzer").apply {
      setCallback(
        object : MediaSession.Callback() {
          override fun onPlay() = natifBasculer()
          override fun onPause() = natifBasculer()
          override fun onSkipToNext() = natifSuivant()
          override fun onSkipToPrevious() = natifPrecedent()
          override fun onSeekTo(pos: Long) = natifPositionner(pos)
          override fun onStop() = natifArreter()
        },
      )
      isActive = true
    }

    creerLeCanal()
    instance = this
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    // L'état que Rust a poussé pendant que le service démarrait.
    enAttente?.let { etat ->
      titre = etat.titre
      artiste = etat.artiste
      enLecture = etat.enLecture
      position = etat.position
      duree = etat.duree
      decoder(etat.pochetteBase64)?.let { pochette = it }
    }

    // Une notification, même vide, doit paraître dans les cinq secondes qui
    // suivent le démarrage, sans quoi Android termine le service.
    startForeground(ID_NOTIFICATION, construire())
    enAttente?.let { appliquerEtat(it) }

    when (intent?.action) {
      ACTION_BASCULER -> natifBasculer()
      ACTION_SUIVANT -> natifSuivant()
      ACTION_PRECEDENT -> natifPrecedent()
    }

    // `START_STICKY` : si le système nous tue faute de mémoire, il nous
    // relance. C'est ce qu'on veut d'un lecteur.
    return START_STICKY
  }

  override fun onDestroy() {
    session.release()
    instance = null
    super.onDestroy()
  }

  override fun onBind(intent: Intent?): IBinder? = null

  // ── État poussé depuis Rust ─────────────────────────────────────────────

  private var titre = ""
  private var artiste = ""
  private var enLecture = false
  private var position = 0L
  private var duree = 0L
  private var pochette: Bitmap? = null

  private fun appliquerEtat(etat: Etat) {
    appliquer(
      etat.titre,
      etat.artiste,
      etat.enLecture,
      etat.position,
      etat.duree,
      decoder(etat.pochetteBase64),
    )
  }

  private fun appliquer(
    titre: String,
    artiste: String,
    enLecture: Boolean,
    position: Long,
    duree: Long,
    pochette: Bitmap?,
  ) {
    this.titre = titre
    this.artiste = artiste
    this.enLecture = enLecture
    this.position = position
    this.duree = duree
    if (pochette != null) this.pochette = pochette

    session.setMetadata(
      MediaMetadata.Builder()
        .putString(MediaMetadata.METADATA_KEY_TITLE, titre)
        .putString(MediaMetadata.METADATA_KEY_ARTIST, artiste)
        .putLong(MediaMetadata.METADATA_KEY_DURATION, duree)
        .also { builder ->
          this.pochette?.let { builder.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, it) }
        }
        .build(),
    )

    session.setPlaybackState(
      PlaybackState.Builder()
        .setActions(
          PlaybackState.ACTION_PLAY
            or PlaybackState.ACTION_PAUSE
            or PlaybackState.ACTION_PLAY_PAUSE
            or PlaybackState.ACTION_SKIP_TO_NEXT
            or PlaybackState.ACTION_SKIP_TO_PREVIOUS
            or PlaybackState.ACTION_SEEK_TO
            or PlaybackState.ACTION_STOP,
        )
        // Vitesse 1 en lecture : c'est elle qui fait avancer la position sur
        // l'écran verrouillé sans qu'on ait à la republier dix fois par
        // seconde.
        .setState(
          if (enLecture) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
          position,
          if (enLecture) 1f else 0f,
        )
        .build(),
    )

    notifier()
  }

  private fun notifier() {
    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    manager.notify(ID_NOTIFICATION, construire())
  }

  private fun construire(): Notification {
    val ouvrir = PendingIntent.getActivity(
      this,
      0,
      Intent(this, MainActivity::class.java),
      PendingIntent.FLAG_IMMUTABLE,
    )

    return Notification.Builder(this, CANAL)
      .setContentTitle(titre.ifEmpty { "Onzer" })
      .setContentText(artiste)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setLargeIcon(pochette)
      .setContentIntent(ouvrir)
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      // Sans cela, la notification se balaie d'un doigt et le service meurt
      // pendant que le son continue — un lecteur devenu impossible à arrêter.
      .setOngoing(enLecture)
      .addAction(action("Précédent", android.R.drawable.ic_media_previous, ACTION_PRECEDENT))
      .addAction(
        if (enLecture) {
          action("Pause", android.R.drawable.ic_media_pause, ACTION_BASCULER)
        } else {
          action("Lire", android.R.drawable.ic_media_play, ACTION_BASCULER)
        },
      )
      .addAction(action("Suivant", android.R.drawable.ic_media_next, ACTION_SUIVANT))
      .setStyle(
        Notification.MediaStyle()
          .setMediaSession(session.sessionToken)
          .setShowActionsInCompactView(0, 1, 2),
      )
      .build()
  }

  private fun action(titre: String, icone: Int, action: String): Notification.Action {
    val intention = PendingIntent.getService(
      this,
      action.hashCode(),
      Intent(this, PlaybackService::class.java).setAction(action),
      PendingIntent.FLAG_IMMUTABLE,
    )

    return Notification.Action.Builder(icone, titre, intention).build()
  }

  private fun creerLeCanal() {
    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    // `IMPORTANCE_LOW` : une notification de lecture ne doit ni sonner ni
    // vibrer. Elle est là pour être consultée, pas pour interrompre.
    manager.createNotificationChannel(
      NotificationChannel(CANAL, "Lecture", NotificationManager.IMPORTANCE_LOW).apply {
        description = "Le morceau en cours et ses commandes"
        setShowBadge(false)
      },
    )
  }

  companion object {
    private const val CANAL = "onzer.lecture"
    private const val ID_NOTIFICATION = 1
    private const val ACTION_BASCULER = "onzer.basculer"
    private const val ACTION_SUIVANT = "onzer.suivant"
    private const val ACTION_PRECEDENT = "onzer.precedent"

    private var instance: PlaybackService? = null

    /**
     * Appelé depuis Rust à chaque changement d'état.
     *
     * Démarre le service au premier appel : tant que rien ne joue, il n'y a
     * aucune raison d'occuper une ligne dans le volet des notifications.
     */
    @JvmStatic
    fun pousser(
      contexte: Context,
      titre: String,
      artiste: String,
      enLecture: Boolean,
      position: Long,
      duree: Long,
      pochetteBase64: String,
    ) {
      // # Pourquoi l'état est mis de côté
      //
      // Au tout premier appel, le service n'existe pas encore : le démarrer
      // est asynchrone. Sans cette mémoire, l'état était simplement perdu, et
      // la notification affichait celui du **coup précédent** — un morceau de
      // retard, une pause affichée en pleine lecture.
      enAttente = Etat(titre, artiste, enLecture, position, duree, pochetteBase64)

      val service = instance
      if (service == null) {
        contexte.startForegroundService(Intent(contexte, PlaybackService::class.java))
        return
      }

      service.appliquerEtat(enAttente!!)
    }

    /** Ce que Rust a poussé en dernier, appliqué dès que le service existe. */
    private var enAttente: Etat? = null

    private data class Etat(
      val titre: String,
      val artiste: String,
      val enLecture: Boolean,
      val position: Long,
      val duree: Long,
      val pochetteBase64: String,
    )

    private fun decoder(base64: String): Bitmap? {
      if (base64.isEmpty()) return null

      return runCatching {
        val octets = Base64.decode(base64, Base64.DEFAULT)
        BitmapFactory.decodeByteArray(octets, 0, octets.size)
      }.getOrNull()
    }
  }

  // ── Vers Rust ───────────────────────────────────────────────────────────

  private external fun natifBasculer()
  private external fun natifSuivant()
  private external fun natifPrecedent()
  private external fun natifPositionner(position: Long)
  private external fun natifArreter()
}
