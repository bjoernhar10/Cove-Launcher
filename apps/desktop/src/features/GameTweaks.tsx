import type { GameTools, Platform, GameTweaks as Tweaks } from "@cove/shared";
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
  /** Text after the description when the tool is missing (default: install it with the package manager). */
  missingText?: string;
}

function Row({ title, description, checked, onChange, missing = false, missingText }: RowProps) {
  const t = useMessages();
  return (
    <div className="flex items-start justify-between gap-8 py-3">
      <div className="min-w-0">
        <p className="text-sm font-medium text-fg">{title}</p>
        <p className="mt-1 text-xs text-fg-muted">{missing ? `${description} ${missingText ?? t.tweaks.notInstalled}` : description}</p>
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

interface Props {
  value: Tweaks;
  onChange: (value: Tweaks) => void;
  /** A Windows game started with Proton (Linux). */
  proton: boolean;
  platform: Platform;
}

/**
 * Options for a game Cove starts itself, saved with the rest of the launch
 * settings. Linux: MangoHud, GameMode, gamescope, FSR and (with Proton)
 * DXVK/VKD3D. Windows: performance mode and the RTSS overlay. macOS: the Metal HUD.
 */
export function GameTweaks({ platform, ...props }: Props) {
  if (platform === "linux") return <LinuxTweaks {...props} />;
  return <OtherTweaks platform={platform} value={props.value} onChange={props.onChange} />;
}

function OtherTweaks({ platform, value, onChange }: { platform: Platform; value: Tweaks; onChange: (value: Tweaks) => void }) {
  const t = useMessages();
  const windows = platform === "windows";
  const tools = useQuery("game-tools", api.system.gameTools);
  const set = (patch: Partial<Tweaks>) => {
    onChange({ ...value, ...patch });
  };
  return (
    <fieldset className="m-0 flex flex-col border-0 p-0">
      <legend className="text-sm font-medium text-fg">{t.tweaks.title}</legend>
      <p className="mt-1 text-xs text-fg-muted">{t.tweaks.hint}</p>
      {windows && tools.status === "loading" ? (
        <LoadingRegion label={t.tweaks.loading}>
          <Skeleton className="mt-3 h-24" />
        </LoadingRegion>
      ) : (
        <div className="mt-2 divide-y divide-border border-y border-border">
          {windows ? (
            <>
              <Row
                title={t.tweaks.performanceMode}
                description={t.tweaks.performanceModeText}
                checked={value.performanceMode}
                onChange={(performanceMode) => {
                  set({ performanceMode });
                }}
              />
              <Row
                title={t.tweaks.rtss}
                description={t.tweaks.rtssText}
                checked={value.rtss}
                missing={tools.status === "success" && !tools.data.rtss}
                missingText={t.tweaks.rtssMissing}
                onChange={(rtss) => {
                  set({ rtss });
                }}
              />
            </>
          ) : (
            <Row
              title={t.tweaks.overlay}
              description={t.tweaks.metalHud}
              checked={value.metalHud}
              onChange={(metalHud) => {
                set({ metalHud });
              }}
            />
          )}
        </div>
      )}
    </fieldset>
  );
}

function LinuxTweaks({ value, onChange, proton }: Omit<Props, "platform">) {
  const t = useMessages();
  const tools = useQuery("game-tools", api.system.gameTools);
  const set = (patch: Partial<Tweaks>) => {
    onChange({ ...value, ...patch });
  };
  const installed: GameTools =
    tools.status === "success" ? tools.data : { mangohud: true, gamemode: true, gamescope: true, rtss: false };

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
            title={t.tweaks.fpsLog}
            description={t.tweaks.fpsLogText}
            checked={value.fpsLog}
            missing={!installed.mangohud}
            onChange={(fpsLog) => {
              set({ fpsLog });
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
