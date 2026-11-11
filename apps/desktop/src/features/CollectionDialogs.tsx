import type { Collection } from "@cove/shared";
import { useState, type SyntheticEvent } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Field, Input } from "../components/ui/Field.tsx";
import { useMessages } from "../i18n/index.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { COLLECTION_NAME_MAX } from "./CollectionMenu.tsx";

interface NameFormProps {
  /** The collection to rename; without it a new one is created. */
  collection: Collection | null;
  onClose: () => void;
  onSaved: (collection: Collection) => void;
}

function NameForm({ collection, onClose, onSaved }: NameFormProps) {
  const t = useMessages();
  const [name, setName] = useState(collection?.name ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const saved = collection ? await api.collections.rename(collection.id, name) : await api.collections.create(name);
      invalidateQueries();
      onSaved(saved);
      onClose();
    } catch (cause: unknown) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  }

  return (
    <form className="flex flex-col gap-5" onSubmit={(e) => void submit(e)} noValidate>
      <Field label={t.collections.name} error={error}>
        {(props) => (
          <Input
            {...props}
            autoFocus
            maxLength={COLLECTION_NAME_MAX}
            placeholder={t.collections.namePlaceholder}
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
          />
        )}
      </Field>
      <div className="flex justify-end gap-2 pt-1">
        <Button onClick={onClose}>{t.common.cancel}</Button>
        <Button type="submit" variant="primary" disabled={busy || !name.trim()}>
          {collection ? t.common.save : t.collections.add}
        </Button>
      </div>
    </form>
  );
}

interface CollectionNameDialogProps extends Partial<Pick<NameFormProps, "onSaved">> {
  open: boolean;
  onClose: () => void;
  /** Rename this collection; `null` creates a new one. */
  collection: Collection | null;
}

/** "Neue Sammlung" and "Sammlung umbenennen". */
export function CollectionNameDialog({ open, onClose, collection, onSaved }: CollectionNameDialogProps) {
  const t = useMessages();
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={collection ? t.collections.rename : t.collections.create}
      description={collection ? undefined : t.collections.createText}
    >
      <NameForm collection={collection} onClose={onClose} onSaved={onSaved ?? (() => undefined)} />
    </Dialog>
  );
}

interface DeleteCollectionDialogProps {
  /** The collection to delete; `null` keeps the dialog closed. */
  collection: Collection | null;
  onClose: () => void;
  onDeleted: () => void;
}

export function DeleteCollectionDialog({ collection, onClose, onDeleted }: DeleteCollectionDialogProps) {
  const t = useMessages();
  const [busy, setBusy] = useState(false);

  async function remove(target: Collection): Promise<void> {
    setBusy(true);
    try {
      await api.collections.remove(target.id);
      toast(t.collections.deleted(target.name));
      onDeleted();
    } catch (cause: unknown) {
      toast(errorMessage(cause), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
      onClose();
    }
  }

  return (
    <Dialog
      open={collection !== null}
      onClose={onClose}
      title={collection ? t.collections.deleteConfirm(collection.name) : t.collections.delete}
      description={t.collections.deleteText}
      footer={
        <>
          <Button onClick={onClose}>{t.common.cancel}</Button>
          <Button
            variant="danger"
            disabled={busy}
            onClick={() => {
              if (collection) void remove(collection);
            }}
          >
            {t.collections.delete}
          </Button>
        </>
      }
    >
      {null}
    </Dialog>
  );
}
