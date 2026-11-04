import type { Game } from "@cove/shared";
import { useState, type CSSProperties } from "react";
import { cx, unlessSet } from "../../lib/cx.ts";

type Aspect = "portrait" | "landscape" | "wide" | "square";

const ASPECTS: Record<Aspect, string> = {
  portrait: "aspect-[3/4]",
  landscape: "aspect-video",
  wide: "aspect-[3/1]",
  square: "aspect-square",
};

function hash(value: string): number {
  let h = 0;
  for (let i = 0; i < value.length; i += 1) h = (h * 31 + value.charCodeAt(i)) >>> 0;
  return h;
}

interface GameCoverProps {
  game: Pick<Game, "id" | "title" | "artwork">;
  aspect?: Aspect;
  /** Which artwork to use. */
  kind?: "cover" | "header";
  /** Show the title on the placeholder (off for tiny thumbnails). */
  showTitle?: boolean;
  titleSize?: "sm" | "base" | "xl";
  /** Variation index, e.g. for screenshots of the same game. */
  variant?: number;
  className?: string;
}

/**
 * Game artwork. Real images when available; otherwise a calm, flat CSS
 * placeholder (one muted tone + one simple shape + the title). No gradients.
 */
export function GameCover({
  game,
  aspect = "portrait",
  kind = "cover",
  showTitle = true,
  titleSize = "base",
  variant = 0,
  className,
}: GameCoverProps) {
  const image = kind === "cover" ? game.artwork.cover : game.artwork.header;
  const base = cx("relative overflow-hidden", unlessSet(className, ["rounded-"], "rounded-lg"), ASPECTS[aspect], className);

  // Remember the URL that failed (offline and not cached yet) and fall back to the placeholder.
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  if (image.url && image.url !== failedUrl) {
    const url = image.url;
    return (
      <img
        src={url}
        alt={image.alt}
        className={cx(base, "w-full bg-skeleton object-cover")}
        loading="lazy"
        decoding="async"
        onError={() => {
          setFailedUrl(url);
        }}
      />
    );
  }

  const h = (hash(game.id) + variant * 7) >>> 0;
  const tone = (h % 8) + 1;
  const shape = (h >>> 3) % 4;
  const style: CSSProperties = {
    backgroundColor: `var(--cove-tone-${String(tone)}-bg)`,
    color: `var(--cove-tone-${String(tone)}-fg)`,
  };

  return (
    <div
      // Without alt text the placeholder is decoration (the title is next to it).
      {...(image.alt ? { role: "img", "aria-label": image.alt } : { "aria-hidden": true })}
      className={base}
      style={{ ...style, containerType: "size" }}
    >
      <div aria-hidden className="absolute inset-0">
        {shape === 0 && <div className="absolute -right-[18cqmin] -bottom-[22cqmin] size-[80cqmin] rounded-full bg-current/15" />}
        {shape === 1 && <div className="absolute -bottom-[40cqmin] left-1/2 size-[80cqmin] -translate-x-1/2 rounded-full border-[6cqmin] border-current/15" />}
        {shape === 2 && (
          <div className="absolute top-[20cqmin] left-[12cqmin] flex w-[60cqmin] flex-col gap-[6cqmin]">
            <div className="h-[5cqmin] rounded-full bg-current/15" />
            <div className="h-[5cqmin] w-3/4 rounded-full bg-current/15" />
            <div className="h-[5cqmin] w-1/2 rounded-full bg-current/15" />
          </div>
        )}
        {shape === 3 && <div className="absolute top-[16cqmin] right-[14cqmin] size-[40cqmin] rotate-12 rounded-lg bg-current/15" />}
      </div>
      {showTitle && (
        <p
          aria-hidden
          className={cx(
            "absolute inset-x-0 bottom-0 p-[8%] leading-tight font-semibold tracking-tight",
            titleSize === "xl" ? "text-xl" : titleSize === "base" ? "text-base" : "text-sm",
          )}
        >
          {game.title}
        </p>
      )}
    </div>
  );
}
