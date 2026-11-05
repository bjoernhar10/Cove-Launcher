import { useMessages } from "../../i18n/index.ts";
import type { LibraryItem } from "@cove/shared";
import { Download, Play, Settings2, Square } from "lucide-react";
import { useState } from "react";
import { api } from "../../api/index.ts";
import { formatPercent } from "../../lib/format.ts";
import { SOURCE_LABELS } from "../../lib/labels.ts";
import { useDownloads } from "../../stores/downloads.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { useRunningGames } from "../../stores/running.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Button, ButtonLink, type ButtonSize } from "../ui/Button.tsx";

interface PlayButtonProps {
  item: LibraryItem;
  size?: ButtonSize;
  iconOnly?: boolean;
  className?: string;
}

/**
 * The primary action for a library game:
 * running → "Läuft" (click stops) · installed → "Spielen" ·
 * manual without executable → "Einrichten" · store game → "Installieren" / download progress.
 */
export function PlayButton({ item, size = "md", iconOnly = false, className }: PlayButtonProps) {
  const t = useMessages();
  const { entry, game } = item;
  const [busy, setBusy] = useState(false);
  const running = useRunningGames((s) => game.id in s.games);
  const download = useDownloads((s) =>
    s.items.find((d) => d.download.gameId === entry.gameId && d.download.queuePosition !== null),
  );
  const label = (text: string) => (iconOnly ? `${game.title}: ${text}` : text);

  async function run(action: () => Promise<void>): Promise<void> {
    setBusy(true);
    try {
      await action();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  if (running) {
    return (
      <Button
        variant="secondary"
        size={size}
        icon={Square}
        iconOnly={iconOnly}
        label={label(t.play.runningStop)}
        title={t.play.stopGame}
        disabled={busy}
        className={className}
        onClick={() =>
          void run(async () => {
            await api.library.stopGame(game.id);
          })
        }
      >
        {t.play.running}
      </Button>
    );
  }

  if (entry.installed) {
    return (
      <Button
        variant="primary"
        size={size}
        icon={Play}
        iconOnly={iconOnly}
        label={iconOnly ? t.play.playTitle(game.title) : t.play.play}
        disabled={busy}
        className={className}
        onClick={() =>
          void run(async () => {
            const result = await api.library.launchGame(game.id);
            toast(
              result.tracked ? t.play.starting(game.title) : t.play.startingVia(game.title, SOURCE_LABELS[entry.source]),
            );
          })
        }
      />
    );
  }

  if (entry.source === "manual") {
    return (
      <ButtonLink
        to={`/library/${game.id}?tab=settings`}
        variant="secondary"
        size={size}
        icon={Settings2}
        iconOnly={iconOnly}
        label={label(t.play.setUp)}
        className={className}
      />
    );
  }

  if (download) {
    const { downloadedBytes, totalBytes, status } = download.download;
    const text =
      status === "handedOff"
        ? t.play.installingIn(SOURCE_LABELS[entry.source])
        : status === "queued" || totalBytes === 0
          ? t.play.queued
          : formatPercent(downloadedBytes / totalBytes);
    return (
      <Button variant="secondary" size={size} icon={Download} iconOnly={iconOnly} label={label(text)} disabled className={className} />
    );
  }

  return (
    <Button
      variant="secondary"
      size={size}
      icon={Download}
      iconOnly={iconOnly}
      label={iconOnly ? t.play.installTitle(game.title) : t.play.install}
      disabled={busy}
      className={className}
      onClick={() =>
        void run(async () => {
          await api.downloads.enqueueDownload(game.id);
          toast(t.play.handedOff(game.title, SOURCE_LABELS[entry.source]));
          invalidateQueries();
        })
      }
    />
  );
}
