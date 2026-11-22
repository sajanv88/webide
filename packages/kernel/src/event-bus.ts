export type Listener<T> = (payload: T) => void;

/** A typed publish/subscribe channel. `Events` maps each event name to its payload. */
export interface EventBus<Events extends object> {
  /** Subscribes; returns the function that unsubscribes. */
  on<K extends keyof Events>(type: K, listener: Listener<Events[K]>): () => void;
  emit<K extends keyof Events>(type: K, payload: Events[K]): void;
}

export interface EventBusOptions {
  /** Called when a listener throws. The other listeners still run. */
  readonly onListenerError?: (error: unknown, type: string) => void;
}

export function createEventBus<Events extends object>(
  options: EventBusOptions = {},
): EventBus<Events> {
  const listeners = new Map<keyof Events, Set<Listener<never>>>();
  const report =
    options.onListenerError ??
    ((error: unknown, type: string) => console.error(`Listener for "${type}" failed`, error));

  return {
    on(type, listener) {
      let set = listeners.get(type);
      if (!set) {
        set = new Set();
        listeners.set(type, set);
      }
      const entry = listener as Listener<never>;
      set.add(entry);
      return () => {
        set.delete(entry);
      };
    },
    emit(type, payload) {
      // Copy, so a listener that unsubscribes while running doesn't skip the next one.
      for (const listener of [...(listeners.get(type) ?? [])]) {
        try {
          (listener as Listener<typeof payload>)(payload);
        } catch (error: unknown) {
          report(error, String(type));
        }
      }
    },
  };
}
