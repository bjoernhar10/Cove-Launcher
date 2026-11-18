import type { FpsLog, LibraryItem } from "@cove/shared";
import { Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatDate, formatNumber, formatPlaytime } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

const fps = (value: number) => formatNumber(Math.round(value));

/** The SVG path of a session's frame rate (0 at the bottom, `top` at the top). */
export function curvePath(samples: number[], top: number, width: number, height: number): string {
  if (samples.length === 0 || top <= 0) return "";
  const step = samples.length > 1 ? width / (samples.length - 1) : 0;
  return samples
    .map((value, i) => {
      const x = i * step;
      const y = height - (Math.min(value, top) / top) * height;
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join(" ");
}

function Curve({ log, title }: { log: FpsLog; title: string }) {
  const t = useMessages();
  const top = Math.max(30, Math.ceil(Math.max(...log.samples) / 30) * 30);
  const width = 600;
  const height = 160;
  const low = height - (Math.min(log.low1Fps, top) / top) * height;
  return (
    <figure className="m-0">
      <figcaption className="sr-only">{title}</figcaption>
      <div className="relative" aria-hidden>
        <span className="num absolute top-0 right-0 text-xs text-fg-subtle">{fps(top)}</span>
        <svg viewBox={`0 0 ${String(width)} ${String(height)}`} preserveAspectRatio="none" className="mt-6 block h-40 w-full border-b border-border">
          <line x1="0" x2={width} y1={low} y2={low} className="stroke-border-strong" strokeDasharray="4 4" vectorEffect="non-scaling-stroke" />
          <path d={curvePath(log.samples, top, width, height)} fill="none" className="stroke-accent" strokeWidth="1.75" vectorEffect="non-scaling-stroke" />
        </svg>
        <div className="mt-2 flex justify-between text-xs text-fg-subtle">
          <span>{t.fps.start}</span>
          <span className="num">{formatPlaytime(Math.max(1, Math.round(log.durationSeconds / 60)))}</span>
        </div>
      </div>
    </figure>
  );
}

/** Linux: frame rates recorded with MangoHud, per session (game option "FPS aufzeichnen"). */
export function FpsHistory({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const id = item.game.id;
  const query = useQuery(`fps/${id}`, () => api.library.fpsLogs(id));
  const [selected, setSelected] = useState<number | null>(null);
  const recording = item.entry.launchOptions.tweaks.fpsLog;

  // Games that never recorded don't get the section (also not while loading).
  if (!recording && (query.status !== "success" || query.data.length === 0)) return null;

  async function clear(): Promise<void> {
    try {
      await api.library.clearFpsLogs(id);
      setSelected(null);
      toast(t.fps.cleared);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      invalidateQueries();
    }
  }

  const logs = query.status === "success" ? query.data : [];
  const current = logs.find((l) => l.id === selected) ?? logs[0];

  return (
    <section aria-labelledby={`fps-${id}`}>
      <div className="flex items-center justify-between gap-4">
        <h2 id={`fps-${id}`} className="text-base font-semibold text-fg">
          {t.fps.title}
        </h2>
        {logs.length > 0 && (
          <Button variant="ghost" size="sm" icon={Trash2} onClick={() => void clear()}>
            {t.fps.clear}
          </Button>
        )}
      </div>
      {query.status === "loading" && (
        <LoadingRegion label={t.fps.loading}>
          <Skeleton className="mt-4 h-40" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && !current && <p className="mt-1 text-sm text-fg-muted">{t.fps.empty}</p>}
      {current && (
        <div className="mt-4 grid grid-cols-1 gap-8 xl:grid-cols-[1fr_320px]">
          <div>
            <dl className="m-0 grid grid-cols-3 gap-4">
              {[
                [t.fps.average, current.averageFps],
                [t.fps.low1, current.low1Fps],
                [t.fps.low01, current.low01Fps],
              ].map(([label, value]) => (
                <div key={label} className="rounded-lg border border-border px-4 py-3">
                  <dt className="text-xs text-fg-muted">{label}</dt>
                  <dd className="num m-0 mt-1 text-lg font-semibold text-fg">{fps(Number(value))}</dd>
                </div>
              ))}
            </dl>
            <div className="mt-6">
              <Curve log={current} title={t.fps.curve(formatDate(current.startedAt))} />
            </div>
            <p className="mt-2 text-xs text-fg-muted">{t.fps.lowLine}</p>
          </div>
          <div>
            <h3 className="text-sm font-medium text-fg">{t.fps.sessions}</h3>
            <ul className="mt-2 max-h-80 overflow-y-auto rounded-lg border border-border">
              {logs.map((log) => {
                const active = log.id === current.id;
                return (
                  <li key={log.id} className="border-b border-border last:border-b-0">
                    <button
                      type="button"
                      aria-pressed={active}
                      onClick={() => {
                        setSelected(log.id);
                      }}
                      className={`flex w-full items-center justify-between gap-4 px-4 py-2 text-left text-sm transition-colors duration-150 ease-out hover:bg-hover ${active ? "bg-active text-fg" : "text-fg-muted"}`}
                    >
                      <span className="min-w-0 truncate">
                        {formatDate(log.startedAt)} · <span className="num">{formatPlaytime(Math.max(1, Math.round(log.durationSeconds / 60)))}</span>
                      </span>
                      <span className="num shrink-0">{t.fps.value(fps(log.averageFps))}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        </div>
      )}
    </section>
  );
}
