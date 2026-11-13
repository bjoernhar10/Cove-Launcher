import type { LibraryItem, StoreSource } from "@cove/shared";
import { useState } from "react";
import { api } from "../api/index.ts";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { cx } from "../lib/cx.ts";
import { SOURCE_LABELS } from "../lib/labels.ts";
import { Library } from "lucide-react";
import { TvCard } from "./TvCard.tsx";

type Group = "all" | "installed" | "favorites" | StoreSource;

function inGroup(item: LibraryItem, group: Group): boolean {
  if (group === "all") return true;
  if (group === "installed") return item.entry.installed;
  if (group === "favorites") return item.entry.favorite;
  return item.entry.source === group;
}

export function TvLibrary() {
  const t = useMessages();
  const library = useQuery("library", api.library.listLibrary);
  const [group, setGroup] = useState<Group>("all");

  if (library.status === "loading") {
    return (
      <LoadingRegion label={t.common.loading}>
        <Skeleton className="h-64" />
      </LoadingRegion>
    );
  }
  if (library.status === "error") return <ErrorState message={library.error} onRetry={library.retry} />;
  const stores = (["steam", "gog", "epic", "amazon", "manual"] as const).filter((s) => library.data.some((i) => i.entry.source === s));
  const groups: { id: Group; label: string }[] = [
    { id: "all", label: t.tv.all },
    { id: "installed", label: t.tv.installed },
    { id: "favorites", label: t.tv.favorites },
    ...stores.map((s) => ({ id: s, label: SOURCE_LABELS[s] })),
  ];
  const items = library.data.filter((i) => inGroup(i, group)).sort((a, b) => a.game.title.localeCompare(b.game.title));

  return (
    <>
      <div role="group" aria-label={t.tv.library} className="mb-6 flex flex-wrap gap-2">
        {groups.map((g) => (
          <button
            key={g.id}
            type="button"
            aria-pressed={group === g.id}
            onClick={() => {
              setGroup(g.id);
            }}
            className={cx(
              "rounded-md px-4 py-2 text-sm transition-colors duration-150 ease-out",
              group === g.id ? "bg-active font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
            )}
          >
            {g.label}
          </button>
        ))}
      </div>
      {items.length === 0 ? (
        <EmptyState icon={Library} title={t.tv.library} description={t.tv.emptyLibrary} />
      ) : (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-6">
          {items.map((item, i) => (
            <li key={item.game.id} className="[content-visibility:auto] [contain-intrinsic-size:auto_260px]">
              <TvCard item={item} className="block rounded-lg" isDefault={i === 0} />
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
