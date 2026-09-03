import { useCallback, useEffect, useState } from "react";

import { ipc, type PlaybackSnapshot } from "@/lib/ipc";

/**
 * État de lecture, synchronisé avec le backend.
 *
 * Deux canaux, pour deux fréquences très différentes :
 *
 * * `playback://state` — changement de morceau ou de file. Rare, charge
 *   complète.
 * * `playback://tick` — position seule, quatre fois par seconde. Réexpédier la
 *   file entière à cette cadence serait absurde, d'où la charge minimale.
 *
 * Les commandes retournent elles-mêmes l'instantané mis à jour : l'interface
 * réagit à l'instant du clic, sans attendre le prochain battement.
 *
 * # La règle qui en découle
 *
 * **Toute commande de lecture doit passer par ce hook.** L'appeler directement
 * depuis un composant compile parfaitement et marche à moitié : l'action a
 * lieu, l'interface l'ignore. Deux appels s'étaient glissés hors du hook, et
 * cliquer un titre de la file changeait la musique sans changer l'affichage.
 */
export function usePlayback() {
  const [state, setState] = useState<PlaybackSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void ipc
      .playbackState()
      .then(setState)
      .catch((cause: unknown) => setError(String(cause)));

    const subscriptions = [
      ipc.onPlaybackState(setState),
      ipc.onPlaybackTick((tick) =>
        setState((previous) =>
          previous === null
            ? previous
            : { ...previous, positionMs: tick.positionMs, isPlaying: tick.isPlaying },
        ),
      ),
    ];

    return () => {
      for (const subscription of subscriptions) {
        void subscription.then((unlisten) => unlisten());
      }
    };
  }, []);

  /** Enveloppe commune : applique le résultat, capture l'erreur. */
  const run = useCallback(async (action: () => Promise<PlaybackSnapshot>) => {
    setError(null);
    try {
      setState(await action());
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  return {
    state,
    error,

    /**
     * Recharge l'état depuis le backend.
     *
     * Filet de sécurité pour les actions qui démarrent la lecture sans passer
     * par ce hook — une playlist générée, par exemple. Le backend émet bien
     * l'événement, mais dépendre d'un seul canal pour faire apparaître le
     * lecteur serait fragile.
     */
    refresh: useCallback(() => {
      void ipc.playbackState().then(setState).catch(() => undefined);
    }, []),
    dismissError: useCallback(() => setError(null), []),

    play: useCallback(
      (trackIds: number[], startAt: number) => run(() => ipc.playTracks(trackIds, startAt)),
      [run],
    ),
    toggle: useCallback(() => run(ipc.togglePlayback), [run]),
    next: useCallback(() => run(ipc.nextTrack), [run]),
    previous: useCallback(() => run(ipc.previousTrack), [run]),
    /**
     * Saute à une position de la file.
     *
     * Passe par `run` comme tout le reste : la commande **rend l'état mis à
     * jour**, et l'appeler sans appliquer ce retour laisse l'interface sur le
     * morceau précédent. C'est exactement ce qui se produisait en cliquant un
     * titre de la file — la musique changeait, la pochette et les paroles non.
     */
    jump: useCallback((index: number) => run(() => ipc.jumpInQueue(index)), [run]),

    /** Ajoute à la file. Rend l'état pour que la file affichée suive. */
    enqueue: useCallback((ids: number[]) => run(() => ipc.enqueueTracks(ids)), [run]),

    /** Insère juste après le morceau en cours. */
    playNext: useCallback((ids: number[]) => run(() => ipc.playNext(ids)), [run]),

    /** Retire de la file, par sa place dans l'ordre de lecture. */
    removeFromQueue: useCallback(
      (position: number) => run(() => ipc.removeFromQueue(position)),
      [run],
    ),

    /** Déplace un morceau dans la file. */
    moveInQueue: useCallback(
      (from: number, to: number) => run(() => ipc.moveInQueue(from, to)),
      [run],
    ),

    seek: useCallback((positionMs: number) => run(() => ipc.seekTo(positionMs)), [run]),
    setVolume: useCallback((volume: number) => run(() => ipc.setVolume(volume)), [run]),
    toggleShuffle: useCallback(
      (shuffle: boolean) => run(() => ipc.setShuffle(shuffle)),
      [run],
    ),
    cycleRepeat: useCallback(
      (current: PlaybackSnapshot["repeat"]) => {
        const next = current === "off" ? "all" : current === "all" ? "one" : "off";
        return run(() => ipc.setRepeat(next));
      },
      [run],
    ),
  };
}
