import type { Collection, GameId } from "@cove/shared";
import { FolderPlus, Plus } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type SyntheticEvent } from "react";
import { createPortal } from "react-dom";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Input } from "../components/ui/Field.tsx";
import { CheckOption } from "../components/ui/Popover.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** Longest collection name (mirrors `MAX_NAME_CHARS` in the Rust core). */
export const COLLECTION_NAME_MAX = 50;

/** Where the menu opens, in viewport pixels. */
export interface MenuAnchor {
  x: number;
  y: number;
}

/** Distance the menu keeps from the window edge, in px. */
const EDGE = 8;

interface CollectionMenuProps {
  game: { id: GameId; title: string };
  anchor: MenuAnchor;
  onClose: () => void;
  /** The button that opened the menu: a press on it is not "outside". */
  trigger?: HTMLElement | null;
}

/**
 * Small floating panel that ticks a game into the user's collections and
 * creates new ones. Opens at the pointer (right-click on a game) or below a
 * button. Closes on Esc, a press outside, scrolling and resizing.
 */
export function CollectionMenu({ game, anchor, onClose, trigger = null }: CollectionMenuProps) {
  const t = useMessages();
  const query = useQuery("collections", api.collections.list);
  const panelRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    closeRef.current = onClose;
  });

  // Keep the panel inside the window, also after it grows.
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    const { width, height } = panel.getBoundingClientRect();
    panel.style.left = `${String(Math.max(EDGE, Math.min(anchor.x, window.innerWidth - width - EDGE)))}px`;
    panel.style.top = `${String(Math.max(EDGE, Math.min(anchor.y, window.innerHeight - height - EDGE)))}px`;
  });

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const inside = (target: EventTarget | null): boolean =>
      target instanceof Node && (panelRef.current?.contains(target) === true || trigger?.contains(target) === true);
    function onPointerDown(event: PointerEvent): void {
      if (!inside(event.target)) closeRef.current();
    }
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === "Escape") closeRef.current();
    }
    function onScroll(event: Event): void {
      if (!inside(event.target)) closeRef.current();
    }
    function onResize(): void {
      closeRef.current();
    }
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    panelRef.current?.focus();
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
      // Closed with the keyboard: go back to where the menu was opened from.
      if (opener?.isConnected && (document.activeElement === null || document.activeElement === document.body)) opener.focus();
    };
  }, [trigger]);

  // Once the list is there, start on its first control.
  const loaded = query.status === "success";
  useEffect(() => {
    const panel = panelRef.current;
    if (loaded && panel && document.activeElement === panel) panel.querySelector<HTMLElement>("input")?.focus();
  }, [loaded]);

  /** Tab stays inside the panel. */
  function onKeyDown(event: ReactKeyboardEvent<HTMLDivElement>): void {
    if (event.key !== "Tab") return;
    const controls = [...event.currentTarget.querySelectorAll<HTMLElement>("input, button:not(:disabled)")];
    const first = controls[0];
    const last = controls.at(-1);
    if (!first || !last) return;
    if (event.shiftKey && (document.activeElement === first || document.activeElement === event.currentTarget)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function toggle(collection: Collection, member: boolean): void {
    query.update((all) =>
      all.map((c) => {
        if (c.id !== collection.id) return c;
        const others = c.gameIds.filter((id) => id !== game.id);
        return { ...c, gameIds: member ? [...others, game.id] : others };
      }),
    );
    api.collections.setGame(collection.id, game.id, member).then(
      () => {
        invalidateQueries();
      },
      (cause: unknown) => {
        toast(errorMessage(cause), "danger");
        invalidateQueries();
      },
    );
  }

  async function create(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const created = await api.collections.create(name);
      await api.collections.setGame(created.id, game.id, true);
      setName("");
    } catch (cause: unknown) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  return createPortal(
    <div
      ref={panelRef}
      role="dialog"
      aria-label={t.collections.forGame(game.title)}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className="fixed z-40 w-64 rounded-lg border border-border bg-raised p-1 opacity-100 transition-opacity duration-150 ease-out starting:opacity-0"
      style={{ left: anchor.x, top: anchor.y }}
    >
      <p className="px-2 py-2 text-xs font-medium text-fg-subtle">{t.collections.title}</p>
      {query.status === "loading" && (
        <LoadingRegion label={t.collections.loading} className="flex flex-col gap-2 px-2 pb-2">
          <Skeleton className="h-4 w-32" />
          <Skeleton className="h-4 w-24" />
        </LoadingRegion>
      )}
      {query.status === "error" && (
        <div className="px-2 pb-2">
          <p className="text-sm text-fg-muted">{query.error}</p>
          <Button size="sm" className="mt-2" onClick={query.retry}>
            {t.common.retry}
          </Button>
        </div>
      )}
      {query.status === "success" &&
        (query.data.length === 0 ? (
          <p className="px-2 pb-2 text-sm text-fg-muted">{t.collections.none}</p>
        ) : (
          <div className="max-h-64 overflow-y-auto">
            {query.data.map((collection) => (
              <CheckOption
                key={collection.id}
                label={collection.name}
                checked={collection.gameIds.includes(game.id)}
                onChange={(member) => {
                  toggle(collection, member);
                }}
              />
            ))}
          </div>
        ))}
      <form className="mt-1 flex gap-1 border-t border-border px-1 pt-2 pb-1" onSubmit={(e) => void create(e)} noValidate>
        <Input
          aria-label={t.collections.newName}
          placeholder={t.collections.newPlaceholder}
          maxLength={COLLECTION_NAME_MAX}
          className="min-w-0 flex-1"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
        />
        <Button type="submit" iconOnly icon={Plus} label={t.collections.add} disabled={busy || !name.trim()} />
      </form>
      {error && (
        <p role="alert" className="px-2 pt-1 pb-2 text-xs text-danger">
          {error}
        </p>
      )}
    </div>,
    document.body,
  );
}

/** "Sammlungen" button of the game page: opens the menu below itself. */
export function CollectionsButton({ game }: { game: { id: GameId; title: string } }) {
  const t = useMessages();
  const query = useQuery("collections", api.collections.list);
  const [open, setOpen] = useState<{ anchor: MenuAnchor; trigger: HTMLElement } | null>(null);
  const count = query.status === "success" ? query.data.filter((c) => c.gameIds.includes(game.id)).length : 0;

  return (
    <>
      <Button
        icon={FolderPlus}
        aria-haspopup="dialog"
        aria-expanded={open !== null}
        onClick={(event) => {
          const rect = event.currentTarget.getBoundingClientRect();
          setOpen(open ? null : { anchor: { x: rect.left, y: rect.bottom + 4 }, trigger: event.currentTarget });
        }}
      >
        {t.collections.title}
        {count > 0 && <span className="num text-xs text-fg-subtle">{count}</span>}
      </Button>
      {open && (
        <CollectionMenu
          game={game}
          anchor={open.anchor}
          trigger={open.trigger}
          onClose={() => {
            setOpen(null);
          }}
        />
      )}
    </>
  );
}
