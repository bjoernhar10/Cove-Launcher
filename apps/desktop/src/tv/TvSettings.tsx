import { Gamepad2, Monitor } from "lucide-react";
import { useNavigate } from "react-router";
import { Button } from "../components/ui/Button.tsx";
import { Switch } from "../components/ui/Field.tsx";
import { Segmented } from "../components/ui/Segmented.tsx";
import { useMessages } from "../i18n/index.ts";
import { useSettings } from "../pages/settings/useSettings.ts";
import { useGamepads } from "../stores/gamepad.ts";

export function TvSettings() {
  const t = useMessages();
  const navigate = useNavigate();
  const { settings, save } = useSettings();
  const pads = useGamepads((s) => s.pads);

  return (
    <div className="flex max-w-3xl flex-col gap-8">
      <Button icon={Monitor} data-nav-default="" onClick={() => void navigate("/")}>
        {t.tv.exit}
      </Button>
      {settings && (
        <>
          <div className="flex items-center justify-between gap-6">
            <div>
              <p className="text-base text-fg">{t.tv.startInTv}</p>
              <p className="mt-1 text-sm text-fg-muted">{t.tv.startInTvHint}</p>
            </div>
            <Switch
              label={t.tv.startInTv}
              checked={settings.general.startInTvMode}
              onChange={(startInTvMode) => {
                save((s) => ({ ...s, general: { ...s.general, startInTvMode } }));
              }}
            />
          </div>
          <div className="flex items-center justify-between gap-6">
            <p className="text-base text-fg">{t.tv.language}</p>
            <Segmented
              label={t.tv.language}
              value={settings.general.language}
              options={[
                { value: "de", label: "Deutsch" },
                { value: "en", label: "English" },
              ]}
              onChange={(language) => {
                save((s) => ({ ...s, general: { ...s.general, language } }));
              }}
            />
          </div>
        </>
      )}
      <section aria-labelledby="tv-controllers">
        <h2 id="tv-controllers" className="mb-3 text-base font-semibold text-fg">
          {t.tv.controllers}
        </h2>
        {pads.length === 0 ? (
          <p className="text-sm text-fg-muted">{t.tv.noControllers}</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {pads.map((pad) => (
              <li key={pad.id} className="flex items-center gap-3 text-base text-fg">
                <Gamepad2 aria-hidden className="text-fg-subtle" />
                {pad.name}
                <span className="text-sm text-fg-muted">{t.tv.kinds[pad.kind]}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
