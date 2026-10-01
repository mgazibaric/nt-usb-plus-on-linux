type Task = () => Promise<unknown>;

/**
 * Runs at most one task per key at a time and keeps only the newest waiting
 * one, so dragging a slider never floods the microphone with stale values.
 */
export class LatestQueue {
  #slots = new Map<string, { next: Task | null }>();
  #running = new Set<Promise<void>>();

  constructor(private onError: (error: unknown) => void) {}

  push(key: string, task: Task): void {
    const busy = this.#slots.get(key);
    if (busy) {
      busy.next = task;
      return;
    }
    const slot: { next: Task | null } = { next: null };
    this.#slots.set(key, slot);
    const done = (async () => {
      try {
        for (let run: Task | null = task; run; ) {
          await run();
          run = slot.next;
          slot.next = null;
        }
      } catch (error) {
        this.onError(error);
      } finally {
        this.#slots.delete(key);
      }
    })();
    this.#running.add(done);
    void done.then(() => this.#running.delete(done));
  }

  /** Resolves once nothing is running or waiting any more. */
  async idle(): Promise<void> {
    while (this.#running.size) await Promise.all(this.#running);
  }
}
