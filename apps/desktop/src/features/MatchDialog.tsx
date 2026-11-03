import type { Game, LibraryItem } from "@cove/shared";
import { Search } from "lucide-react";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, CoveError } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Badge, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Input } from "../components/ui/Field.tsx";
import { useCatalogSearch } from "../hooks/useCatalog.ts";
import { useDebounced } from "../hooks/useDebounced.ts";
import { formatPercent } from "../lib/format.ts";
import { CatalogSetupState } from "../pages/CataloguePage.tsx";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

interface MatchDialogProps {
  item: LibraryItem;
  open: boolean;
  onClose: () => void;
}

function Candidate({ game, score, busy, onPick }: { game: Game; score?: number; busy: boolean; onPick: () => void }) {
  const facts = [game.releaseDate?.slice(0, 4), game.developer].filter(Boolean).join(" · ");
  return (
    <li className="flex items-center gap-3 border-b border-border px-4 py-3 last:border-b-0">
      <GameCover game={game} aspect="portrait" showTitle={false} className="w-10 shrink-0 rounded-md" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium text-fg">{game.title}</p>
        <p className="truncate text-xs text-fg-muted">{facts || "Keine weiteren Angaben"}</p>
      </div>
      {score !== undefined && <Badge className="num">{formatPercent(score)}</Badge>}
      <Button size="sm" disabled={busy} onClick={onPick}>
        Zuordnen
      </Button>
    </li>
  );
}

/**
 * Lets the user confirm or pick the catalogue entry for a library game.
 * Shows stored suggestions (if matching was unsure) plus a free catalogue search.
 */
export function MatchDialog({ item, open, onClose }: MatchDialogProps) {
  const gameId = item.game.id;
  const [query, setQuery] = useState(item.game.title);
  const [busy, setBusy] = useState(false);
  const debounced = useDebounced(query.trim(), 350);
  const searching = debounced.length > 1 && debounced !== item.game.title;
  const candidates = useQuery({
    queryKey: ["library", "match", gameId],
    queryFn: () => api.library.getMatchCandidates(gameId),
    enabled: open,
  });
  const search = useCatalogSearch(searching && open ? debounced : "");
  const notConfigured = search.error instanceof CoveError && search.error.kind === "catalogNotConfigured";

  async function run(action: () => Promise<unknown>, message: string): Promise<void> {
    setBusy(true);
    try {
      await action();
      toast(message);
      invalidateQueries();
      onClose();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  const link = (catalog: Game) => {
    void run(() => api.library.linkCatalog(gameId, catalog.id), `„${catalog.title}“ zugeordnet, Metadaten und Cover übernommen`);
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Katalog-Zuordnung"
      description={`Welcher Katalog-Eintrag gehört zu „${item.game.title}“? Cove übernimmt dann Beschreibung, Genres und Cover. Startdatei und Spielzeit bleiben unverändert.`}
      className="w-[640px]"
      footer={
        <>
          {item.entry.matchStatus === "pending" && (
            <Button disabled={busy} onClick={() => void run(() => api.library.rejectMatch(gameId), "Vorschläge verworfen, keine Zuordnung")}>
              Keins davon
            </Button>
          )}
          <Button onClick={onClose}>Abbrechen</Button>
        </>
      }
    >
      <div className="relative mb-4">
        <Search aria-hidden className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-fg-subtle" />
        <Input
          aria-label="Im Katalog suchen"
          className="pl-9"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
          }}
        />
      </div>

      {notConfigured ? (
        <CatalogSetupState />
      ) : searching ? (
        <section aria-label="Suchergebnisse">
          {search.isPending && <ListSkeleton />}
          {search.isError && <ErrorState message={errorMessage(search.error)} onRetry={() => void search.refetch()} />}
          {search.isSuccess && search.data.items.length === 0 && (
            <p className="py-8 text-center text-sm text-fg-muted">Keine Treffer für „{debounced}“.</p>
          )}
          {search.isSuccess && search.data.items.length > 0 && (
            <ul className="rounded-lg border border-border">
              {search.data.items.map((game) => (
                <Candidate key={game.id} game={game} busy={busy} onPick={() => { link(game); }} />
              ))}
            </ul>
          )}
        </section>
      ) : (
        <section aria-label="Vorschläge">
          {candidates.isPending && <ListSkeleton />}
          {candidates.isError && (
            <ErrorState message={errorMessage(candidates.error)} onRetry={() => void candidates.refetch()} />
          )}
          {candidates.isSuccess && candidates.data.length === 0 && (
            <p className="py-8 text-center text-sm text-fg-muted">
              Keine gespeicherten Vorschläge. Such oben nach dem passenden Spiel.
            </p>
          )}
          {candidates.isSuccess && candidates.data.length > 0 && (
            <ul className="rounded-lg border border-border">
              {candidates.data.map((c) => (
                <Candidate key={c.game.id} game={c.game} score={c.score} busy={busy} onPick={() => { link(c.game); }} />
              ))}
            </ul>
          )}
        </section>
      )}
    </Dialog>
  );
}

function ListSkeleton() {
  return (
    <LoadingRegion label="Lädt" className="flex flex-col gap-2">
      {[0, 1, 2].map((i) => (
        <Skeleton key={i} className="h-16" />
      ))}
    </LoadingRegion>
  );
}
