import { localized, useMessages } from "../../i18n/index.ts";
import type { PluginInfo, PluginManifestPreview, PluginPermission } from "@cove/shared";
import { ChevronDown, FolderOpen, Plus, Puzzle, ShieldCheck, Trash2 } from "lucide-react";
import { useState, type SyntheticEvent } from "react";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { Dialog } from "../../components/ui/Dialog.tsx";
import { EmptyState, ErrorState } from "../../components/ui/Feedback.tsx";
import { Field, Input, Switch } from "../../components/ui/Field.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { PERMISSION_LABELS } from "../../lib/labels.ts";
import { usePlugins } from "../../stores/plugins.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Panel, SettingsSkeleton } from "./parts.tsx";

function PermissionList({ permissions }: { permissions: PluginPermission[] }) {
  const t = useMessages();
  if (permissions.length === 0) {
    return <p className="text-sm text-fg-muted">{t.pluginsTab.noPermissions}</p>;
  }
  return (
    <ul className="flex flex-col gap-3">
      {permissions.map((p) => (
        <li key={p} className="flex gap-3">
          <ShieldCheck aria-hidden className="mt-1 shrink-0 text-fg-subtle" />
          <div>
            <p className="text-sm font-medium text-fg">{PERMISSION_LABELS[p].title}</p>
            <p className="text-sm text-fg-muted">{PERMISSION_LABELS[p].description}</p>
          </div>
        </li>
      ))}
    </ul>
  );
}

/** Like a browser extension: the plugin only runs after the user agreed. */
function PermissionDialog({
  plugin,
  confirmLabel,
  onConfirm,
  onClose,
}: {
  plugin: Pick<PluginManifestPreview, "name" | "version" | "author" | "permissions"> | null;
  confirmLabel: string;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const t = useMessages();
  return (
    <Dialog
      open={plugin !== null}
      onClose={onClose}
      title={plugin ? t.pluginsTab.addConfirm(plugin.name) : ""}
      description={plugin ? t.pluginsTab.addDescription(plugin.version, plugin.author) : undefined}
      footer={
        <>
          <Button onClick={onClose}>{t.common.cancel}</Button>
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void onConfirm().finally(() => {
                setBusy(false);
              });
            }}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {plugin && (
        <>
          <PermissionList permissions={plugin.permissions} />
          <p className="mt-5 text-xs text-fg-subtle">
            {t.pluginsTab.sandboxNote}
          </p>
        </>
      )}
    </Dialog>
  );
}

function InstallDialog({ open, onClose, onInstalled }: { open: boolean; onClose: () => void; onInstalled: () => void }) {
  const t = useMessages();
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<PluginManifestPreview | null>(null);

  function close(): void {
    setPath("");
    setError(null);
    setPreview(null);
    onClose();
  }

  async function inspect(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    try {
      setPreview(await api.plugins.inspectPlugin(path));
    } catch (e: unknown) {
      setError(errorMessage(e));
    }
  }

  if (preview) {
    return (
      <PermissionDialog
        plugin={preview}
        confirmLabel={preview.alreadyInstalled ? t.pluginsTab.update : t.play.install}
        onClose={close}
        onConfirm={async () => {
          try {
            await api.plugins.installPlugin(path);
            toast(t.pluginsTab.installed(preview.name));
            onInstalled();
            close();
          } catch (e: unknown) {
            toast(errorMessage(e), "danger");
          }
        }}
      />
    );
  }

  return (
    <Dialog open={open} onClose={close} title={t.pluginsTab.install} description={t.pluginsTab.installHint}>
      <form className="flex flex-col gap-4" onSubmit={(e) => void inspect(e)} noValidate>
        <Field label={t.addGame.folder} error={error}>
          {(props) => (
            <Input
              {...props}
              autoFocus
              placeholder={t.pluginsTab.folderPlaceholder}
              value={path}
              onChange={(e) => {
                setPath(e.target.value);
                setError(null);
              }}
            />
          )}
        </Field>
        <div className="flex justify-end gap-2">
          <Button type="button" onClick={close}>
            {t.common.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={path.trim() === ""}>
            {t.detail.next}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

const STATUS_TEXT: Record<PluginInfo["status"], string> = localized((m) => m.pluginsTab.status);

function PluginRow({ plugin, onChanged, onReview }: { plugin: PluginInfo; onChanged: () => void; onReview: () => void }) {
  const t = useMessages();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<unknown>, message?: string): Promise<void> {
    setBusy(true);
    try {
      await action();
      if (message) toast(message);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      onChanged();
    }
  }

  return (
    <li className="border-b border-border px-4 py-4 last:border-b-0">
      <div className="flex items-start gap-4">
        <span aria-hidden className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-sidebar text-fg-subtle">
          <Puzzle />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-fg">
            {plugin.name} {plugin.version && <span className="num ml-1 text-xs font-normal text-fg-subtle">v{plugin.version}</span>}
          </p>
          {plugin.description && <p className="mt-1 text-sm text-fg-muted">{plugin.description}</p>}
          <p className={plugin.status === "invalid" ? "mt-1 text-xs text-danger" : "mt-1 text-xs text-fg-subtle"}>
            {plugin.status === "invalid" ? plugin.error : `${STATUS_TEXT[plugin.status]} · ${t.pluginsTab.by(plugin.author)}`}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {plugin.status === "needsApproval" && (
            <Button variant="primary" size="sm" onClick={onReview}>
              {t.pluginsTab.review}
            </Button>
          )}
          {(plugin.status === "active" || plugin.status === "disabled") && (
            <Switch
              label={t.pluginsTab.enable(plugin.name)}
              checked={plugin.status === "active"}
              disabled={busy}
              onChange={(on) => void run(() => api.plugins.setPluginEnabled(plugin.id, on))}
            />
          )}
          {plugin.removable && (
            <Button
              variant="ghost"
              size="sm"
              iconOnly
              icon={Trash2}
              label={t.settings.removePath(plugin.name)}
              disabled={busy}
              onClick={() => void run(() => api.plugins.uninstallPlugin(plugin.id), t.pluginsTab.removed(plugin.name))}
            />
          )}
        </div>
      </div>
      {plugin.status !== "invalid" && (
        <div className="mt-3 pl-14">
          <button
            type="button"
            aria-expanded={open}
            className="inline-flex items-center gap-1 text-xs font-medium text-fg-muted hover:text-fg"
            onClick={() => {
              setOpen((o) => !o);
            }}
          >
            {t.pluginsTab.permissions(plugin.permissions.length)}
            <ChevronDown aria-hidden className={open ? "rotate-180 transition-transform duration-150 ease-out" : "transition-transform duration-150 ease-out"} />
          </button>
          {open && (
            <div className="mt-3">
              <PermissionList permissions={plugin.permissions} />
            </div>
          )}
        </div>
      )}
    </li>
  );
}

export function PluginsTab() {
  const t = useMessages();
  const query = useQuery("plugins", api.plugins.listPlugins);
  const [installing, setInstalling] = useState(false);
  const [review, setReview] = useState<PluginInfo | null>(null);

  function changed(): void {
    invalidateQueries();
    void usePlugins.getState().reload();
  }

  return (
    <Panel title={t.settings.tabs.plugins} description={t.pluginsTab.description}>
      <div className="mb-4 flex flex-wrap gap-2">
        <Button
          variant="primary"
          icon={Plus}
          onClick={() => {
            setInstalling(true);
          }}
        >
          {t.pluginsTab.install}
        </Button>
        <Button
          icon={FolderOpen}
          onClick={() => {
            api.plugins.openPluginFolder().catch((error: unknown) => {
              toast(errorMessage(error), "danger");
            });
          }}
        >
          {t.pluginsTab.openFolder}
        </Button>
      </div>
      {query.status === "loading" && <SettingsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="rounded-lg border border-border">
          <EmptyState
            icon={Puzzle}
            title={t.pluginsTab.emptyTitle}
            description={t.pluginsTab.emptyText}
            compact
          />
        </div>
      )}
      {query.status === "success" && query.data.length > 0 && (
        <ul className="rounded-lg border border-border">
          {query.data.map((plugin) => (
            <PluginRow
              key={plugin.id}
              plugin={plugin}
              onChanged={changed}
              onReview={() => {
                setReview(plugin);
              }}
            />
          ))}
        </ul>
      )}
      <InstallDialog
        open={installing}
        onClose={() => {
          setInstalling(false);
        }}
        onInstalled={changed}
      />
      <PermissionDialog
        plugin={review}
        confirmLabel={t.pluginsTab.allow}
        onClose={() => {
          setReview(null);
        }}
        onConfirm={async () => {
          if (!review) return;
          try {
            await api.plugins.approvePlugin(review.id);
            toast(t.pluginsTab.active(review.name));
            setReview(null);
            changed();
          } catch (error: unknown) {
            toast(errorMessage(error), "danger");
          }
        }}
      />
    </Panel>
  );
}
