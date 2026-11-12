import type { GameTools, GameTweaks as Tweaks } from "@cove/shared";
import { api } from "../api/index.ts";
import { LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Input, Switch } from "../components/ui/Field.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";

interface RowProps {
  title: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** The tool is not installed: the switch only turns it off. */
  missing?: boolean;
}

function Row({ title, description, checked, onChange, missing = false }: RowProps) {
  const t = useMessages();
  return (
    <div className="flex items-start justify-between gap-8 py-3">
      <div className="min-w-0">
        <p className="text-sm font-medium text-fg">{title}</p>
        <p className="mt-1 text-xs text-fg-muted">{missing ? `${description} ${t.tweaks.notInstalled}` : description}</p>
      </div>
      <div className="shrink-0 pt-1">
        <Switch label={title} checked={checked} disabled={missing && !checked} onChange={onChange} />
      </div>
    </div>
  );
}

/** A whole number from a text field; empty or nonsense = null. */
function pixels(text: string): number | null {
  const value = Number.parseInt(text, 10);
  return Number.isFinite(value) && value > 0 ? value : null;
}

/**
 * Linux: MangoHud, GameMode, gamescope, FSR and (with Proton) DXVK/VKD3D for a
 * game Cove starts itself. Saved with the rest of the launch settings.
 */
export function GameTweaks({ value, onChange, proton }: { value: Tweaks; onChange: (value: Tweaks) => void; proton: boolean }) {
  const t = useMessages();
  const tools = useQuery("game-tools", api.system.gameTools);
  const set = (patch: Partial<Tweaks>) => {
    onChange({ ...value, ...patch });
  };
  const installed: GameTools = tools.status === "success" ? tools.data : { mangohud: true, gamemode: true, gamescope: true };

  return (
    <fieldset className="m-0 flex flex-col border-0 p-0">
      <legend className="text-sm font-medium text-fg">{t.tweaks.title}</legend>
      <p className="mt-1 text-xs text-fg-muted">{t.tweaks.hint}</p>
      {tools.status === "loading" ? (
        <LoadingRegion label={t.tweaks.loading}>
          <Skeleton className="mt-3 h-40" />
        </LoadingRegion>
      ) : (
        <div className="mt-2 divide-y divide-border border-y border-border">
          <Row
            title="MangoHud"
            description={t.tweaks.mangohud}
            checked={value.mangohud}
            missing={!installed.mangohud}
            onChange={(mangohud) => {
              set({ mangohud });
            }}
          />
          <Row
            title="GameMode"
            description={t.tweaks.gamemode}
            checked={value.gamemode}
            missing={!installed.gamemode}
            onChange={(gamemode) => {
              set({ gamemode });
            }}
          />
          <div>
            <Row
              title="gamescope"
              description={t.tweaks.gamescope}
              checked={value.gamescope}
              missing={!installed.gamescope}
              onChange={(gamescope) => {
                set({ gamescope });
              }}
            />
            {value.gamescope && (
              <div className="flex flex-wrap items-end gap-6 pb-4">
                <div className="flex flex-col gap-2">
                  <span className="text-xs text-fg-muted">{t.tweaks.resolution}</span>
                  <div className="flex items-center gap-2">
                    <Input
                      inputMode="numeric"
                      className="num w-24"
                      aria-label={t.tweaks.width}
                      placeholder="1920"
                      value={value.gamescopeWidth ?? ""}
                      onChange={(e) => {
                        set({ gamescopeWidth: pixels(e.target.value) });
                      }}
                    />
                    <span aria-hidden className="text-fg-subtle">
                      ×
                    </span>
                    <Input
                      inputMode="numeric"
                      className="num w-24"
                      aria-label={t.tweaks.height}
                      placeholder="1080"
                      value={value.gamescopeHeight ?? ""}
                      onChange={(e) => {
                        set({ gamescopeHeight: pixels(e.target.value) });
                      }}
                    />
                  </div>
                </div>
                <label className="flex h-9 items-center gap-3 text-sm text-fg">
                  <Switch
                    label={t.tweaks.fullscreen}
                    checked={value.gamescopeFullscreen}
                    onChange={(gamescopeFullscreen) => {
                      set({ gamescopeFullscreen });
                    }}
                  />
                  <span aria-hidden>{t.tweaks.fullscreen}</span>
                </label>
              </div>
            )}
          </div>
          <Row
            title="AMD FSR"
            description={value.gamescope ? t.tweaks.fsrGamescope : proton ? t.tweaks.fsrProton : t.tweaks.fsrNeedsGamescope}
            checked={value.fsr}
            onChange={(fsr) => {
              set({ fsr });
            }}
          />
          {proton && (
            <>
              <Row
                title="DXVK"
                description={t.tweaks.dxvk}
                checked={value.dxvk}
                onChange={(dxvk) => {
                  set({ dxvk });
                }}
              />
              <Row
                title="VKD3D-Proton"
                description={t.tweaks.vkd3d}
                checked={value.vkd3d}
                onChange={(vkd3d) => {
                  set({ vkd3d });
                }}
              />
            </>
          )}
        </div>
      )}
    </fieldset>
  );
}
