import type { LibraryItem } from "@cove/shared";
import { useEffect, useRef, useState } from "react";
import { api } from "../api/index.ts";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Textarea } from "../components/ui/Field.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatRelative } from "../lib/format.ts";
import { errorMessage } from "../stores/toasts.ts";

/** Longest note (mirrors `db::notes::MAX_CHARS`). */
export const NOTE_MAX_CHARS = 20_000;
/** Typing pauses this long before the note is stored. */
const SAVE_AFTER_MS = 800;

type SaveState = { kind: "idle" } | { kind: "saving" } | { kind: "saved"; at: string | null } | { kind: "failed"; message: string };

/** "Notizen" on the overview tab: the user's own words on the game, stored while typing. */
export function GameNotes({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const gameId = item.game.id;
  const query = useQuery(`note/${gameId}`, () => api.library.note(gameId));
  const [draft, setDraft] = useState<string | null>(null);
  const [save, setSave] = useState<SaveState>({ kind: "idle" });
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<string | null>(null);

  async function store(text: string): Promise<void> {
    pending.current = null;
    setSave({ kind: "saving" });
    try {
      const note = await api.library.setNote(gameId, text);
      setSave({ kind: "saved", at: note?.updatedAt ?? null });
    } catch (error: unknown) {
      setSave({ kind: "failed", message: errorMessage(error) });
    }
  }

  // Leaving the page stores what was typed last.
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
      if (pending.current !== null) void api.library.setNote(gameId, pending.current).catch(() => undefined);
    },
    [gameId],
  );

  function change(text: string): void {
    setDraft(text);
    pending.current = text;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => void store(text), SAVE_AFTER_MS);
  }

  if (query.status === "loading") {
    return (
      <LoadingRegion label={t.notes.loading}>
        <Skeleton className="h-32" />
      </LoadingRegion>
    );
  }
  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  // While typing, the draft wins over what a background sync brings.
  const value = draft ?? query.data?.text ?? "";
  const updatedAt = save.kind === "saved" ? save.at : (query.data?.updatedAt ?? null);

  return (
    <section aria-labelledby="game-notes">
      <h2 id="game-notes" className="mb-3 text-base font-semibold text-fg">
        {t.notes.title}
      </h2>
      <Textarea
        aria-labelledby="game-notes"
        aria-describedby="game-notes-state"
        rows={5}
        maxLength={NOTE_MAX_CHARS}
        placeholder={t.notes.placeholder}
        value={value}
        onChange={(e) => {
          change(e.target.value);
        }}
        onBlur={() => {
          if (pending.current === null) return;
          if (timer.current) clearTimeout(timer.current);
          void store(pending.current);
        }}
      />
      <p id="game-notes-state" aria-live="polite" className={save.kind === "failed" ? "mt-2 text-xs text-danger" : "mt-2 text-xs text-fg-muted"}>
        {save.kind === "saving"
          ? t.notes.saving
          : save.kind === "failed"
            ? save.message
            : updatedAt
              ? t.notes.saved(formatRelative(updatedAt))
              : t.notes.hint}
      </p>
    </section>
  );
}
