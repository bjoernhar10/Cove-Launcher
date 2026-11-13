import { Pause, Play, X } from "lucide-react";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { Button } from "../components/ui/Button.tsx";
import { ProgressBar } from "../components/ui/Feedback.tsx";
import { useMessages } from "../i18n/index.ts";
import { formatBytes, formatPercent } from "../lib/format.ts";
import { useDownloads } from "../stores/downloads.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

export function TvDownloads() {
  const t = useMessages();
  const items = useDownloads((s) => s.items).filter((d) => d.download.queuePosition !== null);

  function run(task: Promise<void>): void {
    task.catch((error: unknown) => {
      toast(errorMessage(error), "danger");
    });
  }

  if (items.length === 0) return <p className="text-base text-fg-muted">{t.tv.noDownloads}</p>;
  return (
    <ul className="flex flex-col">
      {items.map(({ download, game }, i) => {
        const fraction = download.totalBytes > 0 ? download.downloadedBytes / download.totalBytes : 0;
        const paused = download.status === "paused";
        return (
          <li key={download.id} className="flex items-center gap-6 border-b border-border py-4">
            <GameCover game={game} showTitle={false} className="w-16 rounded-md" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-base font-medium text-fg">{game.title}</p>
              <ProgressBar value={fraction} max={1} label={formatPercent(fraction)} className="mt-2" />
              <p className="num mt-1 text-xs text-fg-subtle">
                {formatPercent(fraction)} · {formatBytes(download.downloadedBytes)} / {formatBytes(download.totalBytes)}
              </p>
            </div>
            <Button
              icon={paused ? Play : Pause}
              {...(i === 0 ? { "data-nav-default": "" } : {})}
              onClick={() => {
                run(paused ? api.downloads.resumeDownload(download.id) : api.downloads.pauseDownload(download.id));
              }}
            >
              {paused ? t.tv.resume : t.tv.pause}
            </Button>
            <Button
              variant="ghost"
              icon={X}
              iconOnly
              label={`${t.tv.cancel}: ${game.title}`}
              onClick={() => {
                run(api.downloads.cancelDownload(download.id));
              }}
            />
          </li>
        );
      })}
    </ul>
  );
}
