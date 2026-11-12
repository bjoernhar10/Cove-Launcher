import type { TimeToBeat as Times } from "@cove/shared";
import { api } from "../api/index.ts";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatHours, formatNumber } from "../lib/format.ts";

/** The length that sorts a game: main story and extras, else whatever is known (seconds). */
export function typicalLength(times: Times): number | null {
  return times.normally ?? times.hastily ?? times.completely;
}

/** "Spieldauer" on a game page: main story, with extras, everything (IGDB player reports). */
export function TimeToBeat({ catalogId }: { catalogId: string }) {
  const t = useMessages();
  const query = useQuery(`time-to-beat/${catalogId}`, () => api.catalog.timeToBeat([catalogId]));

  return (
    <section aria-labelledby={`ttb-${catalogId}`}>
      <h2 id={`ttb-${catalogId}`} className="mb-3 text-base font-semibold text-fg">
        {t.timeToBeat.title}
      </h2>
      {query.status === "loading" && (
        <LoadingRegion label={t.timeToBeat.loading}>
          <Skeleton className="h-28" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && <Rows times={query.data[catalogId] ?? null} />}
    </section>
  );
}

function Rows({ times }: { times: Times | null }) {
  const t = useMessages();
  if (!times) return <p className="rounded-lg border border-border px-4 py-3 text-sm text-fg-muted">{t.timeToBeat.none}</p>;
  const rows: [string, number | null][] = [
    [t.timeToBeat.hastily, times.hastily],
    [t.timeToBeat.normally, times.normally],
    [t.timeToBeat.completely, times.completely],
  ];
  return (
    <>
      <dl className="m-0 rounded-lg border border-border px-4 text-sm">
        {rows.map(([label, seconds]) => (
          <div key={label} className="flex justify-between gap-4 border-b border-border py-3 last:border-b-0">
            <dt className="text-fg-subtle">{label}</dt>
            <dd className="num m-0 text-fg">{seconds === null ? "—" : formatHours(seconds)}</dd>
          </div>
        ))}
      </dl>
      <p className="num mt-2 text-xs text-fg-muted">{t.timeToBeat.source(formatNumber(times.count))}</p>
    </>
  );
}
