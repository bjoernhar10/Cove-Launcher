import { FolderOpen, RotateCcw } from "lucide-react";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { ErrorState } from "../../components/ui/Feedback.tsx";
import { Select, SettingRow, Switch } from "../../components/ui/Field.tsx";
import { useMessages } from "../../i18n/index.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Group, Panel, SettingsSkeleton } from "./parts.tsx";
import { useSettings } from "./useSettings.ts";

const KEEP = [3, 5, 10, 20, 50];

/** Settings → Spielstände: automatic backups, where they go, how many stay. */
export function BackupsTab() {
  const t = useMessages();
  const { settings, error, retry, save } = useSettings();
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (!settings) return <SettingsSkeleton />;
  const backups = settings.backups;

  async function pick(): Promise<void> {
    try {
      const picked = await api.system.pickFolder(backups.directory || null);
      if (!picked) return;
      const info = await api.system.folderInfo(picked);
      if (!info.writable) {
        toast(t.install.notWritable, "danger");
        return;
      }
      save((s) => ({ ...s, backups: { ...s.backups, directory: info.path } }));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  return (
    <Panel title={t.backups.title} description={t.backups.hint}>
      <Group>
        <SettingRow title={t.backups.auto} description={t.backups.autoHint}>
          <Switch
            label={t.backups.auto}
            checked={backups.enabled}
            onChange={(enabled) => {
              save((s) => ({ ...s, backups: { ...s.backups, enabled } }));
            }}
          />
        </SettingRow>
        <SettingRow title={t.backups.folder} description={backups.directory || t.backups.folderDefault}>
          <div className="flex gap-2">
            {backups.directory && (
              <Button
                variant="ghost"
                size="sm"
                iconOnly
                icon={RotateCcw}
                label={t.backups.folderReset}
                onClick={() => {
                  save((s) => ({ ...s, backups: { ...s.backups, directory: "" } }));
                }}
              />
            )}
            <Button size="sm" icon={FolderOpen} onClick={() => void pick()}>
              {t.backups.folderPick}
            </Button>
          </div>
        </SettingRow>
        <SettingRow title={t.backups.keep} description={t.backups.keepHint}>
          <Select
            className="w-32"
            aria-label={t.backups.keep}
            value={String(backups.keep)}
            onChange={(value) => {
              save((s) => ({ ...s, backups: { ...s.backups, keep: Number(value) } }));
            }}
            options={[...new Set([...KEEP, backups.keep])].sort((a, b) => a - b).map((n) => ({ value: String(n), label: t.backups.keepCount(n) }))}
          />
        </SettingRow>
      </Group>
      <p className="mt-4 text-xs text-fg-muted">{t.backups.source}</p>
    </Panel>
  );
}
