/**
 * Global rate limit towards IGDB (4 req/s for the whole proxy, across all
 * isolates and data centres). A single Durable Object hands out time slots;
 * callers wait until their slot before sending the request.
 */
export const IGDB_INTERVAL_MS = 250;
/** Callers never wait longer than this; beyond it the proxy answers 503. */
export const MAX_WAIT_MS = 8000;

export class IgdbGate {
  private nextSlot = 0;

  constructor(
    _state: DurableObjectState,
    _env: unknown,
    private readonly now: () => number = Date.now,
  ) {}

  /** `POST /acquire` → `{ waitMs }` (or 503 when the queue is too long). */
  fetch(): Response {
    const now = this.now();
    const slot = Math.max(now, this.nextSlot);
    const waitMs = slot - now;
    if (waitMs > MAX_WAIT_MS) {
      return Response.json({ waitMs }, { status: 503 });
    }
    this.nextSlot = slot + IGDB_INTERVAL_MS;
    return Response.json({ waitMs });
  }
}
