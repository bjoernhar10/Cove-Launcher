import { useMessages } from "../../i18n/index.ts";
import type { Game } from "@cove/shared";
import { Bookmark, ChartColumn, CornerDownLeft, Plus, Search, type LucideIcon } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { api } from "../../api/index.ts";
import { useDebounced } from "../../hooks/useDebounced.ts";
import { cx } from "../../lib/cx.ts";
import { useUi } from "../../stores/ui.ts";
import { GameCover } from "../game/GameCover.tsx";
import { Dialog } from "../ui/Dialog.tsx";
import { Kbd } from "../ui/Feedback.tsx";
import { MAIN_NAV, SETTINGS_NAV } from "./nav.ts";

interface Command {
  id: string;
  /** Heading the command is listed under (consecutive commands share it). */
  group: string;
  label: string;
  hint?: string;
  icon?: LucideIcon;
  game?: Game;
  run: () => void;
}

function PaletteBody({ onClose }: { onClose: () => void }) {
  const t = useMessages();
  const navigate = useNavigate();
  const openAddGame = useUi((s) => s.openAddGame);
  const [query, setQuery] = useState("");
  const [games, setGames] = useState<Game[]>([]);
  /** Game or catalogue id → library game id. */
  const [owned, setOwned] = useState<Map<string, string>>(new Map());
  const [activeIndex, setActiveIndex] = useState(0);
  const debounced = useDebounced(query, 250);
  const listId = useId();
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    let cancelled = false;
    const catalogHits = debounced.trim().length > 1
      ? api.catalog.searchCatalog(debounced).then((page) => page.items, () => [])
      : Promise.resolve([]);
    void Promise.all([catalogHits, api.library.listLibrary().catch(() => [])]).then(([catalog, library]) => {
      if (cancelled) return;
      // Catalogue entries linked to a library game open the library page.
      setOwned(
        new Map(
          library.flatMap((i): [string, string][] =>
            i.entry.catalogId ? [[i.game.id, i.game.id], [i.entry.catalogId, i.game.id]] : [[i.game.id, i.game.id]],
          ),
        ),
      );
      const q = debounced.trim().toLowerCase();
      // Own games first (including manual ones the catalogue does not know), then catalogue hits.
      const own = q ? library.map((i) => i.game).filter((g) => g.title.toLowerCase().includes(q)) : [];
      const ownIds = new Set(library.flatMap((i) => [i.game.id, i.entry.catalogId ?? ""]));
      setGames([...own, ...catalog.filter((g) => !ownIds.has(g.id))].slice(0, 8));
    });
    return () => {
      cancelled = true;
    };
  }, [debounced]);

  const commands = useMemo<Command[]>(() => {
    const go = (to: string) => () => {
      void navigate(to);
      onClose();
    };
    const q = query.trim().toLowerCase();
    const gameCommands: Command[] = q
      ? games.map((game) => ({
          id: `game-${game.id}`,
          group: t.palette.games,
          label: game.title,
          hint: owned.has(game.id) ? t.palette.inLibrary : t.palette.catalogue,
          game,
          run: go(owned.has(game.id) ? `/library/${owned.get(game.id) ?? game.id}` : `/catalogue/${game.id}`),
        }))
      : [];
    const nav: Command[] = [...MAIN_NAV, SETTINGS_NAV].map((item) => ({
      id: `nav-${item.to}`,
      group: t.palette.navigation,
      label: t.palette.goTo(item.label),
      icon: item.icon,
      run: go(item.to),
    }));
    nav.push({ id: "nav-/wishlist", group: t.palette.navigation, label: t.palette.goTo(t.wishlist.title), icon: Bookmark, run: go("/wishlist") });
    nav.push({ id: "nav-/stats", group: t.palette.navigation, label: t.palette.goTo(t.stats.title), icon: ChartColumn, run: go("/stats") });
    const actions: Command[] = [
      {
        id: "add-game",
        group: t.palette.actions,
        label: t.palette.addGame,
        icon: Plus,
        run: () => {
          onClose();
          openAddGame();
        },
      },
    ];
    const matches = (c: Command) => !q || c.label.toLowerCase().includes(q);
    return [...gameCommands, ...nav.filter(matches), ...actions.filter(matches)];
  }, [games, owned, query, navigate, onClose, openAddGame, t]);

  const safeIndex = Math.min(activeIndex, Math.max(0, commands.length - 1));
  const active = commands[safeIndex];

  useEffect(() => {
    listRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [safeIndex]);

  return (
    <div className="flex flex-col">
      <div className="flex h-12 items-center gap-3 border-b border-border px-4">
        <Search aria-hidden className="text-fg-subtle" />
        <input
          autoFocus
          role="combobox"
          aria-expanded
          aria-controls={listId}
          aria-activedescendant={active ? `${listId}-${active.id}` : undefined}
          aria-label={t.palette.search}
          placeholder={t.topbar.searchPlaceholder}
          className="h-full flex-1 bg-transparent text-base text-fg outline-none placeholder:text-fg-subtle"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActiveIndex(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActiveIndex((safeIndex + 1) % Math.max(commands.length, 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActiveIndex((safeIndex - 1 + commands.length) % Math.max(commands.length, 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              active?.run();
            }
          }}
        />
        <Kbd>Esc</Kbd>
      </div>
      <ul ref={listRef} id={listId} role="listbox" aria-label={t.palette.results} className="max-h-96 overflow-y-auto p-2">
        {commands.length === 0 && <li className="px-3 py-8 text-center text-sm text-fg-muted">{t.palette.noResults(query)}</li>}
        {commands.map((command, index) => {
          const showGroup = index === 0 || commands[index - 1]?.group !== command.group;
          const Icon = command.icon;
          const selected = index === safeIndex;
          return (
            <li key={command.id} role="presentation">
              {showGroup && <p className="px-3 pt-3 pb-2 text-xs font-medium text-fg-subtle first:pt-1">{command.group}</p>}
              <div
                id={`${listId}-${command.id}`}
                role="option"
                aria-selected={selected}
                tabIndex={-1}
                onMouseMove={() => {
                  setActiveIndex(index);
                }}
                onClick={command.run}
                onKeyDown={undefined}
                className={cx("flex h-10 cursor-pointer items-center gap-3 rounded-md px-3 text-sm", selected ? "bg-active text-fg" : "text-fg-muted")}
              >
                {command.game ? (
                  <GameCover game={command.game} aspect="square" showTitle={false} className="w-6 rounded-md" />
                ) : (
                  Icon && <Icon aria-hidden className="text-fg-subtle" />
                )}
                <span className="min-w-0 flex-1 truncate">{command.label}</span>
                {command.hint && <span className="text-xs text-fg-subtle">{command.hint}</span>}
                {selected && <CornerDownLeft aria-hidden className="text-fg-subtle" />}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export function CommandPalette() {
  const t = useMessages();
  const open = useUi((s) => s.paletteOpen);
  const setOpen = useUi((s) => s.setPaletteOpen);
  const close = useMemo(() => () => { setOpen(false); }, [setOpen]);
  return (
    <Dialog open={open} onClose={close} title={t.palette.title} bare className="mt-[12vh] mb-auto w-[600px]">
      {open && <PaletteBody onClose={close} />}
    </Dialog>
  );
}
