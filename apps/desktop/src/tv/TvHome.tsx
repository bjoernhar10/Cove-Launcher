import { api } from "../api/index.ts";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { Gamepad2 } from "lucide-react";
import { TvRow } from "./TvCard.tsx";

export function TvHome() {
  const t = useMessages();
  const library = useQuery("library", api.library.listLibrary);
  const collections = useQuery("collections", api.collections.list);

  if (library.status === "loading") {
    return (
      <LoadingRegion label={t.common.loading}>
        <Skeleton className="h-64" />
      </LoadingRegion>
    );
  }
  if (library.status === "error") return <ErrorState message={library.error} onRetry={library.retry} />;
  const items = library.data;
  if (items.length === 0) return <EmptyState icon={Gamepad2} title={t.tv.home} description={t.tv.emptyHome} />;

  const recent = items
    .filter((i) => i.entry.lastPlayedAt)
    .sort((a, b) => (b.entry.lastPlayedAt ?? "").localeCompare(a.entry.lastPlayedAt ?? ""))
    .slice(0, 12);
  const byTitle = [...items].sort((a, b) => a.game.title.localeCompare(b.game.title));
  const installed = byTitle.filter((i) => i.entry.installed).slice(0, 24);
  const favorites = byTitle.filter((i) => i.entry.favorite).slice(0, 24);
  const groups = (collections.data ?? []).map((c) => ({ id: c.id, name: c.name, items: byTitle.filter((i) => c.gameIds.includes(i.game.id)) }));

  return (
    <>
      <TvRow title={t.tv.continue} items={recent.length > 0 ? recent : byTitle.slice(0, 12)} first />
      <TvRow title={t.tv.installed} items={installed} />
      <TvRow title={t.tv.favorites} items={favorites} />
      {groups.map((g) => (
        <TvRow key={g.id} title={g.name} items={g.items} />
      ))}
    </>
  );
}
