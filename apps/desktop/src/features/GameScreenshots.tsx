import type { LibraryItem, Screenshot } from "@cove/shared";
import { ChevronLeft, ChevronRight, ExternalLink, Image } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatDate } from "../lib/format.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** "Screenshots" tab of a library game: what Steam, the Game Bar and the game itself saved. */
export function GameScreenshots({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const query = useQuery(`screenshots/${item.game.id}`, () => api.library.screenshots(item.game.id));
  const [shown, setShown] = useState<number | null>(null);

  if (query.status === "loading") {
    return (
      <LoadingRegion label={t.screenshots.loading} className="grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-4">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="aspect-video" />
        ))}
      </LoadingRegion>
    );
  }
  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  const shots = query.data;
  if (shots.length === 0) {
    return <EmptyState icon={Image} title={t.screenshots.emptyTitle} description={t.screenshots.emptyText} />;
  }

  return (
    <section aria-label={t.screenshots.title}>
      <p className="num mb-4 text-sm text-fg-muted">{t.screenshots.count(shots.length)}</p>
      <ul className="grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-4" aria-label={t.screenshots.title}>
        {shots.map((shot, index) => (
          <li key={shot.url}>
            <button
              type="button"
              aria-label={t.screenshots.open(shot.fileName)}
              className="block w-full overflow-hidden rounded-lg border border-border transition-colors duration-150 ease-out hover:border-border-strong"
              onClick={() => {
                setShown(index);
              }}
            >
              <img src={shot.url} alt="" loading="lazy" decoding="async" className="aspect-video w-full bg-surface object-cover" />
            </button>
            <p className="mt-2 truncate text-xs text-fg-subtle">
              {formatDate(shot.takenAt)} · {t.screenshots.sources[shot.source]}
            </p>
          </li>
        ))}
      </ul>
      {shown !== null && shots[shown] && (
        <Viewer
          shots={shots}
          index={shown}
          onIndex={setShown}
          onClose={() => {
            setShown(null);
          }}
        />
      )}
    </section>
  );
}

function Viewer({ shots, index, onIndex, onClose }: { shots: Screenshot[]; index: number; onIndex: (index: number) => void; onClose: () => void }) {
  const t = useMessages();
  const shot = shots[index];
  if (!shot) return null;
  const step = (by: number) => {
    onIndex((index + by + shots.length) % shots.length);
  };

  return (
    <Dialog open onClose={onClose} title={shot.fileName} description={`${formatDate(shot.takenAt)} · ${t.screenshots.sources[shot.source]}`} className="w-[1100px]">
      {/* Arrow keys page through (the dialog keeps the focus inside). */}
      <div
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") step(-1);
          if (e.key === "ArrowRight") step(1);
        }}
      >
        <img src={shot.url} alt={shot.fileName} className="max-h-[70vh] w-full rounded-lg border border-border bg-surface object-contain" />
        <div className="mt-4 flex items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <Button size="sm" iconOnly icon={ChevronLeft} label={t.screenshots.previous} disabled={shots.length < 2} onClick={() => {
                step(-1);
              }} />
            <span className="num text-sm text-fg-muted">{t.screenshots.position(index + 1, shots.length)}</span>
            <Button size="sm" iconOnly icon={ChevronRight} label={t.screenshots.next} disabled={shots.length < 2} onClick={() => {
                step(1);
              }} />
          </div>
          <Button
            size="sm"
            icon={ExternalLink}
            onClick={() => {
              api.library.openScreenshot(shot.url).catch((error: unknown) => {
                toast(errorMessage(error), "danger");
              });
            }}
          >
            {t.screenshots.openExternal}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
