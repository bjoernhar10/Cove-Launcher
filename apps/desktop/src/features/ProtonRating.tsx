import type { LibraryItem } from "@cove/shared";
import { ExternalLink } from "lucide-react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Badge, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatNumber } from "../lib/format.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** Linux only: ProtonDB's rating of the game, with a link to its reports. */
export function ProtonRating({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const rating = useQuery(`proton/${item.game.id}`, () => api.library.protonRating(item.game.id));
  const heading = `proton-${item.game.id}`;

  return (
    <section aria-labelledby={heading}>
      <h2 id={heading} className="mb-3 text-base font-semibold text-fg">
        {t.game.protonTitle}
      </h2>
      {rating.status === "loading" && (
        <LoadingRegion label={t.game.protonLoading} className="flex flex-col gap-2">
          <Skeleton className="h-5 w-24" />
          <Skeleton className="h-5 w-56" />
        </LoadingRegion>
      )}
      {rating.status === "error" && (
        <>
          <p className="text-sm text-fg-muted">{rating.error}</p>
          <Button className="mt-3" onClick={rating.retry}>
            {t.common.retry}
          </Button>
        </>
      )}
      {rating.status === "success" && rating.data === null && <p className="text-sm text-fg-muted">{t.game.protonNone}</p>}
      {rating.status === "success" && rating.data !== null && (
        <div className="rounded-lg border border-border px-4 py-3">
          <div className="flex flex-wrap items-center gap-3">
            <Badge className="text-fg">{t.game.protonTier[rating.data.tier].label}</Badge>
            <span className="num text-xs text-fg-subtle">{t.game.protonReports(rating.data.reports, formatNumber(rating.data.reports))}</span>
          </div>
          <p className="mt-2 text-sm text-fg">{t.game.protonTier[rating.data.tier].text}</p>
          {item.entry.source !== "steam" && <p className="mt-1 text-xs text-fg-subtle">{t.game.protonSteamVersion}</p>}
          <Button
            icon={ExternalLink}
            className="mt-3"
            onClick={() => {
              api.system.openExternal(`https://www.protondb.com/app/${encodeURIComponent(rating.data?.steamAppId ?? "")}`).catch((error: unknown) => {
                toast(errorMessage(error), "danger");
              });
            }}
          >
            {t.game.protonOpen}
          </Button>
        </div>
      )}
    </section>
  );
}
