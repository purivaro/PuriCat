import { CircleAlert, Copy, Info, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Spinner } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { RowPage, TableRef } from "../../lib/types";
import { blocker, type Cell, cellValue, isEdited, isInserted, isTruncated, type Pending, rowKey, typedValue } from "./pending";

interface Props {
  /** The cell to show; `null` keeps the dialog closed. */
  cell: Cell | null;
  onClose: () => void;
  connectionId: string;
  table: TableRef;
  page: RowPage;
  pending: Pending;
  /** The grid takes edits (an editable table, and no save in flight). */
  editable: boolean;
  /**
   * Stage a new value. `original` is the cell's whole stored value: for a cell the page shows
   * shortened, that is what makes it editable at all.
   */
  onApply: (cell: Cell, original: string | null, value: string | null) => void;
}

/** What is known about the cell's stored value. */
type Stored =
  | { status: "loading" }
  | { status: "error"; message: string }
  /** `partial`: only a beginning is known, so the value can be read but not edited. */
  | { status: "ready"; value: string | null; partial: "no" | "cut" | "noKey" };

/**
 * One cell, whole: a long text, a JSON document, a value with line breaks. A cell the page
 * shows shortened is fetched in full by the row's primary key. An edit made here is staged
 * like any other — nothing is written until Save.
 */
export function CellDialog({ cell, onClose, ...rest }: Props) {
  return (
    <Dialog.Root open={cell !== null} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-cell-dialog
          onOpenAutoFocus={(e) => e.preventDefault()}
          // What was typed here should not be lost to a stray click beside the dialog.
          onInteractOutside={(e) => e.preventDefault()}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(84vh,620px)] w-[min(94vw,780px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening: each cell starts from its own stored value. */}
          {cell && <Body key={`${cell.r}:${cell.c}`} cell={cell} onClose={onClose} {...rest} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Body({ cell, onClose, connectionId, table, page, pending, editable, onApply }: Props & { cell: Cell }) {
  const { t, i18n } = useTranslation();
  const { r, c } = cell;
  const column = page.columns[c]!;
  const inserted = isInserted(page, r);
  const shortened = !inserted && isTruncated(page, r, c);
  const key = inserted ? null : rowKey(page, r);

  const [stored, setStored] = useState<Stored>(() =>
    !shortened
      ? { status: "ready", value: inserted ? null : (page.rows[r]![c] ?? null), partial: "no" }
      : key
        ? { status: "loading" }
        : { status: "ready", value: page.rows[r]![c] ?? null, partial: "noKey" },
  );
  // What the box holds. `null` is SQL NULL, which an empty box cannot say for a text column.
  const staged = cellValue(page, pending, r, c);
  const [text, setText] = useState<string | null>(() => (inserted || isEdited(pending, r, c) ? (staged ?? null) : null));
  const [touched, setTouched] = useState(false);

  useEffect(() => {
    if (stored.status !== "loading" || !key) return;
    let stale = false;
    ipc
      .fetchCell(connectionId, table, key, column.name)
      .then((content) => !stale && setStored({ status: "ready", value: content.value, partial: content.cut ? "cut" : "no" }))
      .catch((e: unknown) => !stale && setStored({ status: "error", message: errorMessage(e) }));
    return () => {
      stale = true;
    };
    // One fetch per opening; the cell does not change while the dialog is open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const ready = stored.status === "ready" ? stored : null;
  // A staged edit (or a new row's value) is what is shown; otherwise the stored value.
  const shown = touched || inserted || isEdited(pending, r, c) ? text : (ready?.value ?? null);
  const binary = column.kind === "binary";
  const reason = blocker(page, pending, r, c);
  const canEdit = editable && !!ready && ready.partial === "no" && !binary && (reason === null || reason === "truncated");
  const original = ready?.value ?? null;
  const baseline = inserted || isEdited(pending, r, c) ? (staged ?? null) : original;
  const changed = canEdit && touched && shown !== baseline;
  const number = new Intl.NumberFormat(i18n.language);

  const edit = (next: string | null) => {
    setTouched(true);
    setText(next);
  };
  const apply = () => {
    if (!changed) return onClose();
    // An emptied box means NULL where the column cannot hold an empty text.
    onApply(cell, original, shown === null ? null : typedValue(column, shown));
  };
  const copy = async () => {
    await clipboard.write(shown ?? "");
    toast.success(t("common.copied"), { id: "cell-copied", duration: 1500 });
  };

  const notice = !ready
    ? null
    : ready.partial === "cut"
      ? t("cell.cutNotice", { shown: number.format(shown?.length ?? 0) })
      : ready.partial === "noKey"
        ? t("cell.noKeyNotice")
        : binary
          ? t("cell.binaryNotice")
          : !editable || canEdit
            ? null
            : t(`table.notEditable.${reason === "truncated" || reason === null ? "column" : reason}`);

  return (
    <>
      <div className="flex shrink-0 items-center gap-3 border-b border-border py-3 pr-3 pl-5">
        <div className="min-w-0 flex-1">
          <Dialog.Title className="truncate text-[15px] font-semibold">{column.name}</Dialog.Title>
          <div data-cell-where className="mt-0.5 truncate font-mono text-xs text-fg-muted">
            {column.dataType}
            {key && ` · ${key.map((k) => `${k.column}=${k.value}`).join(", ")}`}
            {inserted && ` · ${t("table.newRow")}`}
          </div>
        </div>
        <Dialog.Close asChild>
          <IconButton label={t("common.close")}>
            <X className="size-4" />
          </IconButton>
        </Dialog.Close>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-3 px-5 py-4">
        {notice && (
          <div data-cell-notice className="flex shrink-0 items-start gap-2 rounded-lg bg-sunken px-3 py-2 text-xs leading-relaxed text-fg-muted">
            <Info className="mt-px size-3.5 shrink-0" />
            <span>{notice}</span>
          </div>
        )}
        {stored.status === "loading" ? (
          <div className="flex flex-1 items-center justify-center gap-2 text-[13px] text-fg-muted">
            <Spinner className="size-4" /> {t("common.loading")}
          </div>
        ) : stored.status === "error" ? (
          <div role="alert" data-cell-error className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
            <span data-selectable className="min-w-0 flex-1 break-words">
              {stored.message}
            </span>
          </div>
        ) : (
          <textarea
            data-cell-value
            data-selectable
            aria-label={column.name}
            autoFocus
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            readOnly={!canEdit}
            value={shown ?? ""}
            placeholder={shown === null ? t("table.null") : undefined}
            onChange={(e) => edit(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canEdit) {
                e.preventDefault();
                apply();
              }
            }}
            className={cn(
              "min-h-0 flex-1 resize-none rounded-lg border border-border-strong bg-surface px-3 py-2 text-[13px] leading-relaxed text-fg",
              "placeholder:text-fg-subtle placeholder:italic focus:border-accent focus:ring-3 focus:ring-accent-soft focus:outline-none",
              column.kind !== "text" && "font-mono text-[12.5px]",
              !canEdit && "bg-sunken/60",
            )}
          />
        )}
      </div>

      <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
        <span data-cell-size className="min-w-0 flex-1 truncate text-xs text-fg-muted">
          {ready && (shown === null ? t("table.null") : t("cell.length", { count: shown.length, formatted: number.format(shown.length) }))}
        </span>
        <Button size="sm" variant="ghost" disabled={!ready} icon={<Copy className="size-3.5" />} onClick={() => void copy()}>
          {t("common.copy")}
        </Button>
        {canEdit && column.nullable && (
          <Button size="sm" variant="ghost" disabled={shown === null} onClick={() => edit(null)}>
            {t("table.setNull")}
          </Button>
        )}
        {canEdit ? (
          <>
            <Dialog.Close asChild>
              <Button size="sm" variant="ghost">
                {t("common.cancel")}
              </Button>
            </Dialog.Close>
            <Button size="sm" variant="primary" data-cell-apply disabled={!changed} onClick={apply}>
              {t("cell.apply")}
              <kbd className="ml-0.5 font-sans text-[11px] opacity-80">⌘↵</kbd>
            </Button>
          </>
        ) : (
          <Dialog.Close asChild>
            <Button size="sm">{t("common.close")}</Button>
          </Dialog.Close>
        )}
      </div>
    </>
  );
}
