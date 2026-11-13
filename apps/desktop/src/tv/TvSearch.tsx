import { useMemo, useState } from "react";
import { api } from "../api/index.ts";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { language } from "../i18n/index.ts";
import { TvCard } from "./TvCard.tsx";

const KEYS = {
  de: ["1234567890", "qwertzuiop", "asdfghjklß", "yxcvbnmäöü"],
  en: ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm-:."],
};

/** Search with an on-screen keyboard: every key is a button the controller can reach. */
export function TvSearch() {
  const t = useMessages();
  const [text, setText] = useState("");
  const library = useQuery("library", api.library.listLibrary);
  const rows = KEYS[language() === "de" ? "de" : "en"];
  const results = useMemo(() => {
    const q = text.trim().toLowerCase();
    if (!q || library.status !== "success") return [];
    return library.data.filter((i) => i.game.title.toLowerCase().includes(q)).slice(0, 18);
  }, [text, library]);

  const key = "flex h-12 min-w-12 items-center justify-center rounded-md border border-border px-3 text-base text-fg transition-colors duration-150 ease-out hover:bg-hover";
  return (
    <div className="flex flex-col gap-8">
      <output aria-label={t.tv.searchLabel} aria-live="polite" className="block h-12 rounded-lg border border-border-strong px-4 py-3 text-base text-fg">
        {text || <span className="text-fg-subtle">{t.tv.searchLabel}</span>}
      </output>
      <div role="group" aria-label={t.tv.keyboard} className="flex flex-col gap-2">
        {rows.map((row, r) => (
          <div key={row} className="flex gap-2">
            {(row.match(/./gu) ?? []).map((char, c) => (
              <button
                key={char}
                type="button"
                className={key}
                {...(r === 1 && c === 0 ? { "data-nav-default": "" } : {})}
                onClick={() => {
                  setText((s) => s + char);
                }}
              >
                {char.toUpperCase()}
              </button>
            ))}
          </div>
        ))}
        <div className="flex gap-2">
          <button type="button" className={`${key} w-64`} onClick={() => { setText((s) => `${s} `); }}>
            {t.tv.space}
          </button>
          <button type="button" className={key} onClick={() => { setText((s) => s.slice(0, -1)); }}>
            {t.tv.delete}
          </button>
          <button type="button" className={key} onClick={() => { setText(""); }}>
            {t.tv.clear}
          </button>
        </div>
      </div>
      {library.status === "loading" && (
        <LoadingRegion label={t.common.loading}>
          <Skeleton className="h-40" />
        </LoadingRegion>
      )}
      {library.status === "error" && <ErrorState message={library.error} onRetry={library.retry} />}
      {text.trim() && library.status === "success" && results.length === 0 && <p className="text-base text-fg-muted">{t.tv.noResults}</p>}
      {results.length > 0 && (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-6">
          {results.map((item) => (
            <li key={item.game.id}>
              <TvCard item={item} className="block rounded-lg" />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
