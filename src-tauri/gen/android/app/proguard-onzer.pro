# Ce que R8 ne peut pas voir
#
# Le service de lecture n'est appelé par aucun code Java : c'est le cœur Rust
# qui, par JNI, cherche sa classe par son nom et sa méthode par sa signature.
# R8 ne voit donc **aucun appelant**, et fait ce qu'il doit faire — il renomme
# `pousser` en `a`, ou la supprime.
#
# Le défaut ne se voit qu'en release, et pas à la compilation : l'application
# démarre, la bibliothèque s'affiche, et c'est au premier morceau lancé qu'elle
# disparaît de l'écran. Le journal du système dit alors exactement ce qui s'est
# passé, mais encore faut-il pouvoir le lire — sur un téléphone dont le
# constructeur chiffre logcat, il faut aller le chercher dans le dropbox :
#
#   java.lang.NoSuchMethodError: no static method
#   "Lcom/loogatoxx/onzer/PlaybackService;.pousser(…)V"
#
# Les méthodes `natif*` survivaient, elles : les règles par défaut préservent
# les méthodes natives. C'est le sens inverse — Java appelé depuis Rust — qui
# n'est protégé par rien.
-keep class com.loogatoxx.onzer.PlaybackService { *; }
-keep class com.loogatoxx.onzer.PlaybackService$* { *; }
