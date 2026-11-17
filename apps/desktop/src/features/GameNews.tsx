import type { GameId, NewsBlock, NewsItem } from "@cove/shared";
import { ChevronDown, ExternalLink, Newspaper } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { cx } from "../lib/cx.ts";
import { formatDate } from "../lib/format.ts";

/** "Patch-Notes" next to a post's title: neutral, like every badge in Cove. */
export function PatchBadge() {
  const t = useMessages();
  return <span className="shrink-0 rounded-md border border-border px-2 text-xs leading-5 text-fg-muted">{t.news.patchNotes}</span>;
}

/** A post's text: headings, paragraphs and lists (plain text, never markup). */
function Body({ blocks }: { blocks: NewsBlock[] }) {
  const parts: ReactNode[] = [];
  let items: string[] = [];
  const flushList = (key: number) => {
    if (items.length === 0) return;
    parts.push(
      <ul key={`l${String(key)}`} className="flex list-disc flex-col gap-1 pl-5">
        {items.map((text, i) => (
          <li key={i}>{text}</li>
        ))}
      </ul>,
    );
    items = [];
  };
  blocks.forEach((block, i) => {
    if (block.kind === "item") {
      items.push(block.text);
      return;
    }
    flushList(i);
    parts.push(
      block.kind === "heading" ? (
        <h4 key={i} className="pt-2 text-sm font-semibold text-fg">
          {block.text}
        </h4>
      ) : (
        <p key={i}>{block.text}</p>
      ),
    );
  });
  flushList(blocks.length);
  return <div className="flex max-w-3xl flex-col gap-3 text-sm text-fg-muted">{parts}</div>;
}

function Post({ post, initiallyOpen }: { post: NewsItem; initiallyOpen: boolean }) {
  const t = useMessages();
  const [open, setOpen] = useState(initiallyOpen);
  const bodyId = useId();
  return (
    <li className="border-b border-border last:border-b-0">
      <h3>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={() => {
            setOpen(!open);
          }}
          className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors duration-150 ease-out hover:bg-hover focus-visible:-outline-offset-2"
        >
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2">
              <span className="truncate text-sm font-medium text-fg">{post.title}</span>
              {post.patchNotes && <PatchBadge />}
            </span>
            <span className="mt-1 block text-xs text-fg-muted">
              <time dateTime={post.publishedAt}>{formatDate(post.publishedAt)}</time>
              {post.author && ` · ${post.author}`}
            </span>
          </span>
          <ChevronDown
            aria-hidden
            className={cx("shrink-0 text-fg-muted transition-transform duration-150 ease-out motion-reduce:transition-none", open && "rotate-180")}
          />
        </button>
      </h3>
      <div id={bodyId} hidden={!open} className="px-4 pb-4">
        {post.blocks.length > 0 ? <Body blocks={post.blocks} /> : <p className="text-sm text-fg-muted">{t.news.noText}</p>}
        {post.url && (
          <Button
            variant="ghost"
            size="sm"
            icon={ExternalLink}
            className="mt-3 -ml-2"
            onClick={() => {
              if (post.url) void api.system.openExternal(post.url);
            }}
          >
            {t.news.readOnSteam}
          </Button>
        )}
      </div>
    </li>
  );
}

/** "Neuigkeiten" on a library game page: the developers' announcements and patch notes on Steam. */
export function GameNews({ gameId, focus }: { gameId: GameId; focus?: string | null }) {
  const t = useMessages();
  const query = useQuery(`news/${gameId}`, () => api.library.gameNews(gameId));
  if (query.status === "loading") {
    return (
      <LoadingRegion label={t.news.loading} className="flex flex-col gap-2">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-16" />
        ))}
      </LoadingRegion>
    );
  }
  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  if (query.data.length === 0) {
    return (
      <div className="rounded-lg border border-border">
        <EmptyState compact icon={Newspaper} title={t.news.emptyTitle} description={t.news.emptyText} />
      </div>
    );
  }
  const opened = query.data.some((p) => p.id === focus) ? focus : query.data[0]?.id;
  return (
    <section aria-label={t.news.title}>
      <ul className="rounded-lg border border-border">
        {query.data.map((post) => (
          <Post key={post.id} post={post} initiallyOpen={post.id === opened} />
        ))}
      </ul>
      <p className="mt-3 text-xs text-fg-subtle">{t.news.source}</p>
    </section>
  );
}
