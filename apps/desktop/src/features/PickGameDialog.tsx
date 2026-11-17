import type { LibraryItem, TimeToBeat } from "@cove/shared";
import { Dices, Shuffle } from "lucide-react";
import { useMemo, useState } from "react";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { PlayButton } from "../components/game/PlayButton.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Field, Select, Switch } from "../components/ui/Field.tsx";
import { Segmented } from "../components/ui/Segmented.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { localized, useMessages } from "../i18n/index.ts";
import { formatHours, formatPlaytime, formatRelative } from "../lib/format.ts";
import { LENGTHS, typicalLength } from "../lib/lengths.ts";
import { DEFAULT_PICK, pickCandidates, pickGame, type PickFilters } from "../lib/pick.ts";
import type { LengthFilter } from "../stores/preferences.ts";
import { useUi } from "../stores/ui.ts";

const LENGTH_LABELS: Record<LengthFilter, string> = localized((m) => m.timeToBeat.lengths);
/** How many shown games "Nochmal" avoids. */
const REMEMBER = 20;

function Result({ item, times }: { item: LibraryItem; times: TimeToBeat | undefined }) {
  const t = useMessages();
  const { game, entry } = item;
  const length = times ? typicalLength(times) : null;
  const facts = [
    entry.playtimeMinutes > 0 ? t.picker.played(formatPlaytime(entry.playtimeMinutes)) : t.picker.neverPlayed,
    entry.lastPlayedAt ? t.picker.lastPlayed(formatRelative(entry.lastPlayedAt)) : null,
    length !== null ? t.picker.length(formatHours(length)) : null,
  ].filter((fact): fact is string => fact !== null);
  return (
    <div aria-live="polite" className="overflow-hidden rounded-lg border border-border">
      <GameCover game={game} kind="header" aspect="landscape" titleSize="xl" className="rounded-none" />
      <div className="p-4">
        <h3 className="truncate text-base font-medium text-fg">{game.title}</h3>
        <p className="mt-1 text-sm text-fg-muted">{facts.join(" · ")}</p>
      </div>
    </div>
  );
}

/** "Was soll ich spielen?": one game at random from the library, with a few filters. */
export function PickGameDialog() {
  const t = useMessages();
  const open = useUi((s) => s.pickerOpen);
  const setOpen = useUi((s) => s.setPickerOpen);
  const close = () => {
    setOpen(false);
  };
  return (
    <Dialog open={open} onClose={close} title={t.picker.title} description={t.picker.subtitle} className="w-[min(36rem,calc(100vw-2rem))]">
      {open && <Picker onClose={close} />}
    </Dialog>
  );
}

function Picker({ onClose }: { onClose: () => void }) {
  const t = useMessages();
  const library = useQuery("library", api.library.listLibrary);
  const collections = useQuery("collections", api.collections.list);
  const [filters, setFilters] = useState<PickFilters>(DEFAULT_PICK);
  const [recent, setRecent] = useState<string[]>([]);
  const items = useMemo(() => (library.status === "success" ? library.data : []), [library.status, library.data]);
  const catalogIds = useMemo(() => items.flatMap((i) => (i.entry.catalogId ? [i.entry.catalogId] : [])), [items]);
  // Lengths come from the catalogue; only asked for once a length is chosen (the core caches them).
  const times = useQuery(filters.length === "any" ? "time-to-beat/off" : `time-to-beat/library/${String(catalogIds.length)}`, () =>
    filters.length === "any" ? Promise.resolve<Record<string, TimeToBeat>>({}) : api.catalog.timeToBeat(catalogIds),
  );
  const known = useMemo(() => (times.status === "success" ? times.data : {}), [times.status, times.data]);
  const groups = useMemo(() => (collections.status === "success" ? collections.data : []), [collections.status, collections.data]);
  const candidates = useMemo(() => pickCandidates(items, filters, known, groups), [items, filters, known, groups]);
  // The game on screen: the last one shown while the filters allow it, else a new pick.
  const [seed] = useState(Math.random);
  const ready = library.status === "success" && (filters.length === "any" || times.status === "success");
  const last = recent.at(-1);
  const shown = ready ? (candidates.find((item) => item.game.id === last) ?? pickGame(candidates, recent, () => seed)) : null;

  function next(): void {
    const seen = shown ? [...recent.filter((id) => id !== shown.game.id), shown.game.id] : recent;
    const game = pickGame(candidates, seen);
    if (game) setRecent([...seen.filter((id) => id !== game.game.id), game.game.id].slice(-REMEMBER));
  }

  function update(patch: Partial<PickFilters>): void {
    setFilters((current) => ({ ...current, ...patch }));
  }

  const error = library.status === "error" ? library : times.status === "error" ? times : null;
  return (
    <div className="flex flex-col gap-5">
      {error?.status === "error" ? (
        <ErrorState message={error.error} onRetry={error.retry} />
      ) : !ready ? (
        <LoadingRegion label={t.picker.loading}>
          <Skeleton className="aspect-video" />
        </LoadingRegion>
      ) : shown ? (
        <Result item={shown} times={shown.entry.catalogId ? known[shown.entry.catalogId] : undefined} />
      ) : (
        <div className="rounded-lg border border-border">
          <EmptyState
            compact
            icon={Dices}
            title={items.length === 0 ? t.picker.emptyLibrary : t.picker.noneTitle}
            description={items.length === 0 ? t.picker.emptyLibraryText : t.picker.noneText}
            actions={
              items.length > 0 ? (
                <Button
                  onClick={() => {
                    setFilters(DEFAULT_PICK);
                  }}
                >
                  {t.picker.reset}
                </Button>
              ) : undefined
            }
          />
        </div>
      )}

      <div className="flex flex-col gap-4">
        <Segmented
          label={t.picker.kind}
          value={filters.kind}
          onChange={(kind) => {
            update({ kind });
          }}
          options={[
            { value: "any", label: t.picker.kinds.any },
            { value: "new", label: t.picker.kinds.new },
            { value: "resume", label: t.picker.kinds.resume },
          ]}
        />
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label={t.timeToBeat.filter}>
            {(props) => (
              <Select
                {...props}
                value={filters.length}
                onChange={(length) => {
                  update({ length });
                }}
                options={(["any", ...LENGTHS.map((l) => l.value)] as const).map((value) => ({ value, label: LENGTH_LABELS[value] }))}
              />
            )}
          </Field>
          {groups.length > 0 && (
            <Field label={t.collections.title}>
              {(props) => (
                <Select
                  {...props}
                  value={filters.collection ?? ""}
                  onChange={(id) => {
                    update({ collection: id === "" ? null : id });
                  }}
                  options={[{ value: "", label: t.picker.allGames }, ...groups.map((c) => ({ value: c.id, label: c.name }))]}
                />
              )}
            </Field>
          )}
        </div>
        <div className="flex items-center justify-between gap-4">
          <span aria-hidden className="text-sm text-fg">
            {t.picker.installedOnly}
          </span>
          <Switch
            checked={filters.installedOnly}
            label={t.picker.installedOnly}
            onChange={(installedOnly) => {
              update({ installedOnly });
            }}
          />
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-border pt-4">
        <Button icon={Shuffle} disabled={candidates.length < 2} onClick={next}>
          {t.picker.again}
        </Button>
        {shown && (
          <>
            <ButtonLink to={`/library/${shown.game.id}`} onClick={onClose}>
              {t.picker.open}
            </ButtonLink>
            <PlayButton item={shown} />
          </>
        )}
      </div>
    </div>
  );
}
