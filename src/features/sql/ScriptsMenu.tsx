import { CircleAlert, FilePlus2, FileText, FolderOpen, Pencil, Search, Trash2 } from "lucide-react";
import { Popover } from "radix-ui";
import { type KeyboardEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, IconButton, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc, toAppError } from "../../lib/ipc";
import type { SavedScript } from "../../lib/types";
import { useWorkspace } from "../../stores/workspace";

/** Closed, showing the list, or asking for the name of a new script on top of it. */
export type ScriptsMode = "closed" | "list" | "name";

/** The backend's limit (`store::SCRIPT_NAME_MAX`). */
const NAME_MAX = 120;
/** A search box is only worth its row once the list no longer fits a glance. */
const SEARCH_FROM = 8;

const fieldClass = cn(
  "h-7 min-w-0 flex-1 rounded-lg border border-border-strong bg-surface px-2 text-xs text-fg placeholder:text-fg-subtle",
  "focus:border-accent focus:ring-3 focus:ring-accent-soft focus:outline-none disabled:opacity-50",
);

const firstLine = (sql: string) => sql.replace(/\s+/g, " ").trim();

interface Props {
  connectionId: string;
  connectionName: string;
  /** The script the tab in front is editing, when it is one. */
  scriptId: number | undefined;
  /** What a new script would be called to start with. */
  suggested: string;
  mode: ScriptsMode;
  onMode: (mode: ScriptsMode) => void;
  /** The editor holds nothing to save. */
  empty: boolean;
  /** Save the editor's text as a new script. Rejects with the backend's reason. */
  onCreate: (name: string) => Promise<void>;
  onOpen: (script: SavedScript) => void;
}

/**
 * The scripts saved for a connection: open one, save the tab's text as a new one, rename,
 * delete. They live in PuriCat's local settings database, exactly as written.
 */
export function ScriptsMenu({ connectionId, connectionName, scriptId, suggested, mode, onMode, empty, onCreate, onOpen }: Props) {
  const { t, i18n } = useTranslation();
  const open = mode !== "closed";
  const [scripts, setScripts] = useState<SavedScript[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [name, setName] = useState("");
  const [nameError, setNameError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [renaming, setRenaming] = useState<{ id: number; name: string; error: string | null } | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);

  // Read afresh at every opening: another tab may have saved or deleted a script.
  useEffect(() => {
    if (!open) return;
    let stale = false;
    setSearch("");
    setRenaming(null);
    setDeleting(null);
    ipc
      .listScripts(connectionId)
      .then((list) => !stale && (setScripts(list), setFailure(null)))
      .catch((e: unknown) => !stale && setFailure(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [open, connectionId]);

  useEffect(() => {
    if (mode !== "name") return;
    setName(suggested);
    setNameError(null);
    // `suggested` is read once, when the name is asked for.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  /** Why a name was refused; a name already in use is said in the user's language. */
  const refusal = (e: unknown, wanted: string) => (toAppError(e).kind === "conflict" ? t("sql.scriptNameTaken", { name: wanted.trim() }) : errorMessage(e));

  const create = async () => {
    if (busy || !name.trim()) return;
    setBusy(true);
    try {
      await onCreate(name);
      onMode("closed");
    } catch (e) {
      setNameError(refusal(e, name));
    } finally {
      setBusy(false);
    }
  };

  const rename = async () => {
    if (!renaming || busy || !renaming.name.trim()) return;
    setBusy(true);
    try {
      const saved = await ipc.renameScript(renaming.id, renaming.name);
      setScripts((list) => list && list.map((s) => (s.id === saved.id ? saved : s)).sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" })));
      useWorkspace.getState().renameScript(saved.id, saved.name);
      setRenaming(null);
    } catch (e) {
      setRenaming((r) => r && { ...r, error: refusal(e, r.name) });
    } finally {
      setBusy(false);
    }
  };

  const remove = async (script: SavedScript) => {
    setBusy(true);
    try {
      await ipc.deleteScript(script.id);
      setScripts((list) => list && list.filter((s) => s.id !== script.id));
      // A tab that was editing it keeps its text and is a plain query tab again.
      useWorkspace.getState().unbindScript(script.id);
      setDeleting(null);
      toast.success(t("sql.scriptDeleted", { name: script.name }), { duration: 2000 });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  // Not while an input method is still composing the name (Thai, Japanese, ...).
  const onEnter = (action: () => void) => (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !e.nativeEvent.isComposing) action();
  };

  const needle = search.trim().toLowerCase();
  const shown = scripts?.filter((s) => !needle || s.name.toLowerCase().includes(needle) || s.sql.toLowerCase().includes(needle)) ?? [];

  return (
    <Popover.Root open={open} onOpenChange={(o) => onMode(o ? "list" : "closed")}>
      <Popover.Trigger asChild>
        <IconButton label={t("sql.scripts")} active={open} data-sql-scripts-toggle>
          <FolderOpen className="size-4" />
        </IconButton>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="end"
          sideOffset={6}
          collisionPadding={12}
          data-sql-scripts
          // The name box takes the keyboard when a name is asked for.
          onOpenAutoFocus={(e) => mode === "name" && e.preventDefault()}
          // Escape first backs out of a rename or a delete that was started.
          onEscapeKeyDown={(e) => {
            if (!renaming && deleting === null) return;
            e.preventDefault();
            setRenaming(null);
            setDeleting(null);
          }}
          className="animate-pop z-[60] flex max-h-[min(70vh,520px)] w-[min(92vw,440px)] flex-col rounded-xl border border-border bg-raised text-[13px] text-fg shadow-pop outline-none"
        >
          <div className="flex shrink-0 items-center gap-2 border-b border-border py-2.5 pr-2.5 pl-3.5">
            <div className="min-w-0 flex-1">
              <div className="font-semibold">{t("sql.scripts")}</div>
              <div className="truncate text-xs text-fg-muted">{t("sql.scriptsOn", { name: connectionName })}</div>
            </div>
            {mode === "list" && (
              <Button size="sm" disabled={empty} title={empty ? t("sql.scriptNothing") : undefined} icon={<FilePlus2 className="size-3.5" />} onClick={() => onMode("name")}>
                {t(scriptId === undefined ? "sql.scriptSaveNew" : "sql.scriptSaveAs")}
              </Button>
            )}
          </div>

          {mode === "name" && (
            <div data-script-name className="shrink-0 border-b border-border bg-sunken/50 px-3.5 py-2.5">
              <label className="block text-xs font-medium text-fg-muted">
                {t("sql.scriptName")}
                <div className="mt-1 flex items-center gap-1.5">
                  <input
                    autoFocus
                    autoComplete="off"
                    autoCorrect="off"
                    autoCapitalize="off"
                    spellCheck={false}
                    maxLength={NAME_MAX}
                    value={name}
                    placeholder={t("sql.scriptNamePlaceholder")}
                    onFocus={(e) => e.target.select()}
                    onChange={(e) => {
                      setName(e.target.value);
                      setNameError(null);
                    }}
                    onKeyDown={onEnter(() => void create())}
                    className={fieldClass}
                  />
                  <Button size="sm" variant="primary" data-script-create loading={busy} disabled={!name.trim()} onClick={() => void create()}>
                    {t("common.save")}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => onMode("list")}>
                    {t("common.cancel")}
                  </Button>
                </div>
              </label>
              {nameError && <Problem>{nameError}</Problem>}
            </div>
          )}

          {scripts && scripts.length >= SEARCH_FROM && (
            <div className="relative shrink-0 border-b border-border px-2 py-1.5">
              <Search className="pointer-events-none absolute top-1/2 left-4 size-3.5 -translate-y-1/2 text-fg-subtle" />
              <input
                aria-label={t("sql.scriptSearch")}
                placeholder={t("sql.scriptSearch")}
                autoComplete="off"
                spellCheck={false}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className={cn(fieldClass, "w-full pl-7")}
              />
            </div>
          )}

          {/* While the first script is being named there is no list to show under the name. */}
          <div className={cn("min-h-0 flex-1 overflow-y-auto p-1.5", mode === "name" && scripts?.length === 0 && "hidden")}>
            {failure ? (
              <Problem>{failure}</Problem>
            ) : !scripts ? (
              <div className="flex items-center gap-2 px-2 py-3 text-xs text-fg-muted">
                <Spinner className="size-3.5" /> {t("common.loading")}
              </div>
            ) : scripts.length === 0 ? (
              <div data-script-empty className="flex flex-col items-center gap-2 px-6 py-7 text-center">
                <div className="flex size-9 items-center justify-center rounded-xl bg-accent-soft text-accent-text">
                  <FileText className="size-4" />
                </div>
                <div className="text-[13px] font-medium">{t("sql.scriptsEmpty")}</div>
                <div className="text-xs leading-relaxed text-fg-muted">{t("sql.scriptsEmptyBody")}</div>
              </div>
            ) : shown.length === 0 ? (
              <div className="px-3 py-5 text-center text-xs text-fg-muted">{t("sql.scriptsNoMatch", { text: search.trim() })}</div>
            ) : (
              <ul className="flex flex-col gap-0.5">
                {shown.map((s) =>
                  renaming?.id === s.id ? (
                    <li key={s.id} className="rounded-lg bg-sunken px-2 py-1.5">
                      <div className="flex items-center gap-1.5">
                        <input
                          autoFocus
                          aria-label={t("sql.scriptName")}
                          autoComplete="off"
                          spellCheck={false}
                          maxLength={NAME_MAX}
                          value={renaming.name}
                          onFocus={(e) => e.target.select()}
                          onChange={(e) => setRenaming({ id: s.id, name: e.target.value, error: null })}
                          onKeyDown={onEnter(() => void rename())}
                          className={fieldClass}
                        />
                        <Button size="sm" variant="primary" loading={busy} disabled={!renaming.name.trim()} onClick={() => void rename()}>
                          {t("sql.scriptRename")}
                        </Button>
                        <Button size="sm" variant="ghost" onClick={() => setRenaming(null)}>
                          {t("common.cancel")}
                        </Button>
                      </div>
                      {renaming.error && <Problem>{renaming.error}</Problem>}
                    </li>
                  ) : deleting === s.id ? (
                    <li key={s.id} data-script-deleting className="flex items-center gap-1.5 rounded-lg bg-danger-soft py-1.5 pr-2 pl-3">
                      <span className="min-w-0 flex-1 text-xs leading-snug break-words text-danger">{t("sql.scriptDeleteAsk", { name: s.name })}</span>
                      <Button size="sm" variant="danger" loading={busy} onClick={() => void remove(s)}>
                        {t("common.delete")}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => setDeleting(null)}>
                        {t("common.cancel")}
                      </Button>
                    </li>
                  ) : (
                    <li key={s.id} data-script-row className="group flex items-center gap-0.5 rounded-lg pr-1 hover:bg-sunken has-focus-visible:bg-sunken">
                      <button
                        type="button"
                        title={s.name}
                        onClick={() => {
                          onMode("closed");
                          onOpen(s);
                        }}
                        className="flex min-w-0 flex-1 flex-col items-start gap-0.5 rounded-lg px-2 py-1.5 text-left outline-none"
                      >
                        <span className="flex w-full items-center gap-1.5">
                          <span className="truncate font-medium">{s.name}</span>
                          {s.id === scriptId && (
                            <span className="shrink-0 rounded bg-accent-soft px-1 py-px text-[10px] font-semibold text-accent-text">{t("sql.scriptThisTab")}</span>
                          )}
                        </span>
                        <span className="flex w-full items-baseline gap-1.5 text-[11px] text-fg-subtle">
                          <span className="shrink-0 tabular-nums">{new Date(s.updatedAt).toLocaleDateString(i18n.language, { dateStyle: "medium" })}</span>
                          <span className="truncate font-mono">{firstLine(s.sql) || t("sql.scriptBlank")}</span>
                        </span>
                      </button>
                      <IconButton
                        label={t("sql.scriptRename")}
                        className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                        onClick={() => {
                          setDeleting(null);
                          setRenaming({ id: s.id, name: s.name, error: null });
                        }}
                      >
                        <Pencil className="size-3.5" />
                      </IconButton>
                      <IconButton
                        label={t("sql.scriptDelete")}
                        className="opacity-0 group-hover:opacity-100 hover:text-danger focus-visible:opacity-100"
                        onClick={() => {
                          setRenaming(null);
                          setDeleting(s.id);
                        }}
                      >
                        <Trash2 className="size-3.5" />
                      </IconButton>
                    </li>
                  ),
                )}
              </ul>
            )}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

function Problem({ children }: { children: string }) {
  return (
    <div role="alert" data-script-problem className="mt-1.5 flex items-start gap-1.5 text-xs leading-snug text-danger">
      <CircleAlert className="mt-px size-3.5 shrink-0" />
      <span className="min-w-0 break-words">{children}</span>
    </div>
  );
}
