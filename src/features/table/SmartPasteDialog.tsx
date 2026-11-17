import { ArrowRight, CircleAlert, ClipboardPaste, FileSpreadsheet, FolderOpen, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton, Segmented, Spinner, StateMessage } from "../../components/ui/primitives";
import { clipboard, parseClipboardTable } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type {
  ApplyResult,
  ColumnMeta,
  ConnectionView,
  FileReadOptions,
  FileTable,
  PasteAnalysis,
  PasteMode,
  PasteRowStatus,
  PickedFile,
  TableRef,
  UniqueKey,
} from "../../lib/types";
import { autoMap, buildRequest, columnLetter, looksLikeHeaders, type Mapping, updatableColumns, usableKeys } from "./smartPaste";

/** Rows one run may hold (`db::write::MAX_STATEMENTS`). */
const RUN_LIMIT = 10_000;
/** Encodings offered for a text file that is not UTF-8, by the label the backend takes. */
const ENCODINGS = ["windows-874", "windows-1252", "windows-1251", "shift_jis", "gbk", "big5", "euc-kr", "utf-16le"];
const SEPARATORS = [",", ";", "\t", "|"];
const SEPARATOR_NAME: Record<string, string> = { ",": "comma", ";": "semicolon", "\t": "tab", "|": "pipe" };

/** Rows listed in the preview. Every row is analysed; this only bounds what is drawn. */
const ROWS_SHOWN = 200;

const TONE: Record<PasteRowStatus, string> = {
  insert: "bg-ok-soft text-ok",
  update: "bg-warn-soft text-warn",
  unchanged: "bg-sunken text-fg-muted",
  unmatched: "bg-info-soft text-info",
  missing_key: "bg-danger-soft text-danger",
  duplicate: "bg-danger-soft text-danger",
  exists: "bg-danger-soft text-danger",
  invalid: "bg-danger-soft text-danger",
};
const COUNT_TONE = { insert: "text-ok", update: "text-warn", unchanged: "text-fg-muted", unmatched: "text-info", problems: "text-danger" };

/** Compact select for the dense mapping rows (the shared input style is full-width). */
const selectClass =
  "h-7 rounded-md border border-border-strong bg-surface px-2 text-xs text-fg hover:border-fg-subtle focus:border-accent focus:outline-none disabled:opacity-50";

interface Props {
  open: boolean;
  onClose: () => void;
  connection: ConnectionView;
  table: TableRef;
  columns: ColumnMeta[];
  onDone: (result: ApplyResult) => void;
}

export function SmartPasteDialog(props: Props) {
  // While a run is in flight the dialog cannot be dismissed (Esc, outside click, the close
  // button): the only way out is "Stop and roll back", so its outcome is always seen.
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={props.open} onOpenChange={(o) => !o && !busy && props.onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-smart-paste
          tabIndex={-1}
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          // A run in progress must be stopped explicitly, not dismissed by a stray click or Esc.
          onInteractOutside={(e) => e.preventDefault()}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(90vh,780px)] w-[min(95vw,900px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening, so every paste starts from the current clipboard (a file is opened from there). */}
          {props.open && <Wizard {...props} onBusy={setBusy} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Wizard({ onClose, connection, table, columns, onDone, onBusy }: Props & { onBusy: (busy: boolean) => void }) {
  const { t, i18n } = useTranslation();
  const production = connection.environment === "production";

  const [matrix, setMatrix] = useState<string[][] | null>(null); // null: the source is being read
  // The rows come from the clipboard unless a file was opened; `table` is null while it loads.
  const [file, setFile] = useState<{ picked: PickedFile; options: FileReadOptions; table: FileTable | null } | null>(null);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const reading = useRef(0);
  const [hasHeaders, setHasHeaders] = useState(false);
  const [nullText, setNullText] = useState(false);
  const [mapping, setMapping] = useState<Mapping>([]);
  const [mode, setMode] = useState<PasteMode>("upsert");
  const [keys, setKeys] = useState<UniqueKey[]>([]);
  const [keyName, setKeyName] = useState<string | null>(null); // null: pick the first usable key
  const [skipUpdate, setSkipUpdate] = useState<Record<string, true>>({});

  const [analysis, setAnalysis] = useState<{ of: string; result: PasteAnalysis } | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [run, setRun] = useState<{ job: string; done: number; total: number } | null>(null);
  const analysing = useRef(0);
  useEffect(() => {
    onBusy(!!run);
    return () => onBusy(false);
  }, [run, onBusy]);
  const previewRef = useRef<HTMLElement>(null);

  const take = useCallback(
    (next: string[][]) => {
      const headers = looksLikeHeaders(next, columns);
      setMatrix(next);
      setHasHeaders(headers);
      setMapping(autoMap(next, headers, columns));
      setAnalysis(null);
      setError(null);
    },
    [columns],
  );

  const read = useCallback(async () => {
    const id = ++reading.current;
    setFile(null);
    setSourceError(null);
    let text = "";
    try {
      text = await clipboard.read();
    } catch {
      /* an empty or non-text clipboard reads as nothing to paste */
    }
    if (id === reading.current) take(parseClipboardTable(text));
  }, [take]);

  const loadFile = useCallback(
    async (picked: PickedFile, options: FileReadOptions) => {
      const id = ++reading.current;
      setFile({ picked, options, table: null });
      setSourceError(null);
      setMatrix(null);
      try {
        const table = await ipc.readTableFile(picked.token, options);
        if (id !== reading.current) return;
        setFile({ picked, options, table });
        // More rows than one run may hold: nothing is offered for mapping, the wizard says why.
        take(table.totalRows > table.rows.length ? [] : table.rows);
      } catch (e) {
        if (id !== reading.current) return;
        setSourceError(errorMessage(e));
        setMatrix([]);
      }
    },
    [take],
  );

  const openFile = async () => {
    try {
      const picked = await ipc.pickTableFile();
      if (picked) await loadFile(picked, {});
    } catch (e) {
      setSourceError(errorMessage(e));
    }
  };

  useEffect(() => {
    void read();
    ipc
      .uniqueKeys(connection.id, table)
      .then(setKeys)
      .catch((e: unknown) => setError(errorMessage(e)));
  }, [read, connection.id, table]);

  // ---- derived set-up ----------------------------------------------------------------
  const available = useMemo(() => usableKeys(keys, mapping), [keys, mapping]);
  // INSERT may run without a key; UPDATE and UPSERT always use one (the first usable by default).
  const picked = available.find((k) => k.name === keyName);
  const chosen = mode === "insert" ? picked : (picked ?? available[0]);
  const key = useMemo(() => chosen?.columns ?? [], [chosen]);
  const updatable = useMemo(() => updatableColumns(mapping, key, columns), [mapping, key, columns]);
  const generatedKey = key.some((k) => columns.find((c) => c.name === k)?.generated);

  const built = useMemo(
    () =>
      matrix &&
      buildRequest({
        mode,
        matrix,
        hasHeaders,
        mapping,
        key,
        update: updatable.filter((c) => !skipUpdate[c]),
        columns,
        nullText,
      }),
    [matrix, mode, hasHeaders, mapping, key, updatable, skipUpdate, columns, nullText],
  );
  const signature = built ? JSON.stringify(built.request) : "";
  const current = analysis?.of === signature ? analysis.result : null;
  const stale = !!analysis && !current;
  const dataRows = built?.request.rows.length ?? 0;
  const mappedCount = mapping.filter(Boolean).length;
  const canPreview = !!built && dataRows > 0 && mappedCount > 0 && !checking && !run;
  const toRun = current ? current.counts.insert + current.counts.update : 0;
  const blocked = !!current && (current.problems.length > 0 || current.counts.problems > 0);

  const preview = async () => {
    if (!built) return;
    const id = ++analysing.current;
    setChecking(true);
    setError(null);
    try {
      const result = await ipc.analyzePaste(connection.id, table, built.request);
      if (id === analysing.current) setAnalysis({ of: signature, result });
    } catch (e) {
      if (id === analysing.current) setError(errorMessage(e));
    } finally {
      if (id === analysing.current) setChecking(false);
    }
  };

  const start = async () => {
    if (!built || !current || blocked || toRun === 0) return;
    const job = crypto.randomUUID();
    setRun({ job, done: 0, total: toRun });
    setError(null);
    try {
      const result = await ipc.applyPaste(connection.id, table, built.request, current.fingerprint, job, (done) =>
        setRun((r) => r && { ...r, done }),
      );
      onDone(result);
    } catch (e) {
      setError(errorMessage(e));
      setRun(null);
      setAnalysis(null); // whatever happened, the next run needs a fresh look at the table
    }
  };

  // The answer appears below the set-up: bring it into view when it arrives.
  useEffect(() => {
    if (analysis) previewRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [analysis]);

  const setTarget = (i: number, name: string | null) =>
    // A table column can receive one pasted column: taking it frees it elsewhere.
    setMapping((m) => m.map((old, j) => (j === i ? name : name && old === name ? null : old)));

  const header = (i: number) => (hasHeaders && matrix?.[0]?.[i]?.trim()) || t("smart.column", { letter: columnLetter(i) });
  const sample = (i: number) => matrix?.[hasHeaders ? 1 : 0]?.[i] ?? "";
  const keyLabel = (k: UniqueKey) => t(k.primary ? "smart.keyPrimary" : "smart.keyUnique", { columns: k.columns.join(", ") });
  const keyAt = built ? key.map((k) => built.request.columns.indexOf(k)) : [];
  const number = new Intl.NumberFormat(i18n.language);
  const tooMany = !!file?.table && file.table.totalRows > file.table.rows.length;
  // The two places rows can come from, offered wherever there are none to work with.
  const sources = (
    <div className="flex items-center gap-2">
      <Button variant={file ? "secondary" : "primary"} icon={<ClipboardPaste className="size-3.5" />} onClick={() => void read()}>
        {t("smart.reread")}
      </Button>
      <Button data-smart-open-file variant={file ? "primary" : "secondary"} icon={<FolderOpen className="size-3.5" />} onClick={() => void openFile()}>
        {t(file ? "smart.openAnother" : "smart.openFile")}
      </Button>
    </div>
  );

  return (
    <>
      <div className="flex shrink-0 items-center gap-3 border-b border-border py-3 pr-3 pl-5">
        <ClipboardPaste className="size-5 shrink-0 text-accent-text" />
        <Dialog.Title className="min-w-0 flex-1 truncate text-[15px] font-semibold">{t("smart.title", { name: table.name })}</Dialog.Title>
        <div className="flex h-7 max-w-56 shrink-0 items-center gap-1.5 rounded-lg border border-border px-1.5 text-xs">
          <EngineTag engine={connection.engine} />
          <span className="truncate font-medium">{connection.name}</span>
          <EnvBadge environment={connection.environment} long />
        </div>
        <IconButton label={t("common.close")} disabled={!!run} onClick={onClose}>
          <X className="size-4" />
        </IconButton>
      </div>

      {file && (
        <div data-smart-file className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-border bg-sunken/50 px-5 py-2 text-[13px]">
          <span className="flex min-w-0 items-center gap-1.5 font-medium">
            <FileSpreadsheet className="size-4 shrink-0 text-fg-muted" />
            <span className="truncate" title={file.picked.name}>
              {file.picked.name}
            </span>
          </span>
          {file.picked.kind === "workbook" && file.table && file.table.sheets.length > 1 && (
            <label className="flex items-center gap-1.5">
              <span className="text-fg-muted">{t("smart.sheet")}</span>
              <select
                data-smart-sheet
                value={file.table.sheet ?? ""}
                disabled={!!run}
                onChange={(e) => void loadFile(file.picked, { ...file.options, sheet: e.target.value })}
                className={cn(selectClass, "max-w-56")}
              >
                {file.table.sheets.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
            </label>
          )}
          {file.picked.kind === "text" && (
            <>
              <label className="flex items-center gap-1.5">
                <span className="text-fg-muted">{t("smart.encoding")}</span>
                <select
                  data-smart-encoding
                  value={file.options.encoding ?? ""}
                  disabled={!!run}
                  onChange={(e) => void loadFile(file.picked, { ...file.options, encoding: e.target.value || undefined })}
                  className={selectClass}
                >
                  <option value="">{t("smart.encodingAuto")}</option>
                  {ENCODINGS.map((label) => (
                    <option key={label} value={label}>
                      {t(`smart.encodingName.${label}`)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex items-center gap-1.5">
                <span className="text-fg-muted">{t("smart.separator")}</span>
                <select
                  data-smart-separator
                  value={file.options.delimiter ?? ""}
                  disabled={!!run}
                  onChange={(e) => void loadFile(file.picked, { ...file.options, delimiter: e.target.value || undefined })}
                  className={selectClass}
                >
                  <option value="">
                    {file.table?.delimiter && !file.options.delimiter
                      ? t("smart.separatorDetected", { name: t(`smart.separatorName.${SEPARATOR_NAME[file.table.delimiter] ?? "other"}`) })
                      : t("smart.separatorAuto")}
                  </option>
                  {SEPARATORS.map((s) => (
                    <option key={s} value={s}>
                      {t(`smart.separatorName.${SEPARATOR_NAME[s]}`)}
                    </option>
                  ))}
                </select>
              </label>
            </>
          )}
          {file.picked.kind === "workbook" && <span className="min-w-0 flex-1 truncate text-xs text-fg-subtle">{t("smart.storedValues")}</span>}
        </div>
      )}

      {matrix === null ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-fg-muted">
          <Spinner /> {t("common.loading")}
        </div>
      ) : sourceError ? (
        <div data-smart-file-error className="flex-1">
          <StateMessage tone="danger" icon={<CircleAlert className="size-5" />} title={t("smart.fileFailed")} body={sourceError} action={sources} />
        </div>
      ) : tooMany ? (
        <div data-smart-too-many className="flex-1">
          <StateMessage
            icon={<TriangleAlert className="size-5" />}
            title={t("smart.tooManyTitle")}
            body={t("smart.tooManyBody", { total: number.format(file?.table?.totalRows ?? 0), limit: number.format(RUN_LIMIT) })}
            action={sources}
          />
        </div>
      ) : matrix.length === 0 ? (
        <div className="flex-1">
          <StateMessage
            icon={file ? <FileSpreadsheet className="size-5" /> : <ClipboardPaste className="size-5" />}
            title={t(file ? "smart.emptyFile" : "smart.emptyTitle")}
            body={t("smart.emptyBody")}
            action={sources}
          />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
          {/* 1 — what was pasted */}
          <section className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <span data-smart-source className="text-[13px] font-medium">
              {file
                ? t("smart.sourceFile", {
                    rows: dataRows,
                    columns: matrix[0]!.length,
                    name: file.table?.sheet ? `${file.picked.name} › ${file.table.sheet}` : file.picked.name,
                  })
                : t("smart.source", { rows: dataRows, columns: matrix[0]!.length })}
              {built && built.blankRows > 0 && (
                <span className="ml-2 font-normal text-fg-subtle">{t("smart.blankRows", { count: built.blankRows })}</span>
              )}
            </span>
            <Check
              label={t("smart.headers")}
              checked={hasHeaders}
              disabled={!!run}
              onChange={(v) => {
                setHasHeaders(v);
                setMapping(autoMap(matrix, v, columns));
              }}
            />
            <Check label={t("smart.nullText")} checked={nullText} disabled={!!run} onChange={setNullText} />
            <span className="flex-1" />
            <Button size="sm" disabled={!!run} icon={<ClipboardPaste className="size-3.5" />} onClick={() => void read()}>
              {t("smart.reread")}
            </Button>
            <Button size="sm" data-smart-open-file disabled={!!run} icon={<FolderOpen className="size-3.5" />} onClick={() => void openFile()}>
              {t("smart.openFile")}
            </Button>
          </section>

          {/* 2 — what to do with it */}
          <section className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-3">
              <Segmented
                label="Mode"
                value={mode}
                onChange={(m) => !run && setMode(m)}
                options={(["insert", "update", "upsert"] as const).map((m) => ({ value: m, label: t(`smart.mode.${m}`) }))}
              />
              <span className="min-w-0 flex-1 text-[13px] text-fg-muted">{t(`smart.modeHint.${mode}`)}</span>
            </div>
            <label className="flex flex-wrap items-center gap-2 text-[13px]">
              <span className="shrink-0 font-medium">{t("smart.key")}</span>
              <select
                data-smart-key
                value={chosen?.name ?? ""}
                disabled={!!run || (mode !== "insert" && available.length === 0)}
                onChange={(e) => setKeyName(e.target.value)}
                className={cn(selectClass, "h-8 max-w-full min-w-56 text-[13px]")}
              >
                {(mode === "insert" || available.length === 0) && <option value="">{t("smart.keyNone")}</option>}
                {available.map((k) => (
                  <option key={k.name} value={k.name}>
                    {keyLabel(k)}
                  </option>
                ))}
              </select>
              {mode !== "insert" && available.length === 0 && <span className="text-xs text-danger">{t("smart.noKey")}</span>}
              {mode === "upsert" && generatedKey && <span className="text-xs text-fg-muted">{t("smart.generatedKey")}</span>}
            </label>
          </section>

          {/* 3 — which pasted column goes where */}
          <section>
            <h3 className="mb-1.5 text-xs font-semibold tracking-wide text-fg-subtle uppercase">{t("smart.columns")}</h3>
            <div className="overflow-hidden rounded-xl border border-border">
              {matrix[0]!.map((_, i) => {
                const target = mapping[i] ?? null;
                const col = columns.find((c) => c.name === target);
                const isKey = !!target && key.includes(target);
                return (
                  <div key={i} className="flex items-center gap-2.5 border-b border-border px-3 py-1 last:border-b-0">
                    <span className="w-6 shrink-0 font-mono text-[11px] text-fg-subtle">{columnLetter(i)}</span>
                    <span className="w-40 shrink-0 truncate text-[13px] font-medium" title={header(i)}>
                      {header(i)}
                    </span>
                    <span className="min-w-0 flex-1 truncate font-mono text-xs text-fg-muted" title={sample(i)}>
                      {sample(i)}
                    </span>
                    <ArrowRight aria-hidden className="size-3.5 shrink-0 text-fg-subtle" />
                    <select
                      aria-label={`${t("smart.tableColumn")}: ${header(i)}`}
                      value={target ?? ""}
                      disabled={!!run}
                      onChange={(e) => setTarget(i, e.target.value || null)}
                      className={cn(selectClass, "w-56 shrink-0", !target && "text-fg-subtle")}
                    >
                      <option value="">{t("smart.skip")}</option>
                      {columns.map((c) => (
                        <option key={c.name} value={c.name}>
                          {c.name} · {c.dataType}
                          {c.editable ? "" : ` (${t("smart.matchOnly")})`}
                        </option>
                      ))}
                    </select>
                    <span className="flex w-20 shrink-0 justify-end">
                      {isKey ? (
                        <span className="rounded bg-accent-soft px-1.5 py-0.5 text-[10px] font-bold text-accent-text uppercase">key</span>
                      ) : mode !== "insert" && col?.editable ? (
                        <Check
                          label={t("smart.updateIt")}
                          checked={!skipUpdate[col.name]}
                          disabled={!!run}
                          onChange={(v) =>
                            setSkipUpdate((s) => {
                              const next = { ...s };
                              if (v) delete next[col.name];
                              else next[col.name] = true;
                              return next;
                            })
                          }
                        />
                      ) : null}
                    </span>
                  </div>
                );
              })}
            </div>
            <p className="mt-2 text-xs leading-relaxed text-fg-subtle">{t("smart.semantics")}</p>
          </section>

          {/* 4 — exactly what would happen */}
          {current && (
            <section ref={previewRef} data-smart-preview className="flex scroll-mb-2 flex-col gap-2">
              {current.problems.length > 0 ? (
                <ul className="flex flex-col gap-1 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
                  {current.problems.map((p) => (
                    <li key={p} className="flex gap-2">
                      <CircleAlert className="mt-0.5 size-4 shrink-0" /> {p}
                    </li>
                  ))}
                </ul>
              ) : (
                <>
                  <div className="flex flex-wrap gap-x-5 gap-y-1 text-[13px]">
                    {(["insert", "update", "unchanged", "unmatched", "problems"] as const).map((k) => (
                      <span key={k} data-smart-count={k} className={cn(current.counts[k] === 0 && "opacity-45")}>
                        <b className={cn("text-[15px] tabular-nums", COUNT_TONE[k])}>{current.counts[k]}</b>{" "}
                        <span className="text-fg-muted">{t(`smart.count.${k}`)}</span>
                      </span>
                    ))}
                  </div>
                  {current.counts.problems > 0 && <p className="text-xs text-danger">{t("smart.fixFirst")}</p>}
                  {toRun === 0 && current.counts.problems === 0 && <p className="text-xs text-fg-muted">{t("smart.nothing")}</p>}
                  <div className="max-h-64 overflow-auto rounded-xl border border-border">
                    <table className="w-full border-separate border-spacing-0 text-xs">
                      <thead>
                        <tr className="[&>th]:sticky [&>th]:top-0 [&>th]:border-b [&>th]:border-border [&>th]:bg-sunken [&>th]:px-2.5 [&>th]:py-1.5 [&>th]:text-left [&>th]:font-semibold">
                          <th className="w-12">{t("smart.row")}</th>
                          <th className="w-24">{t("smart.result")}</th>
                          <th>{key.length ? key.join(", ") : built!.request.columns[0]}</th>
                          <th>{t("smart.detail")}</th>
                        </tr>
                      </thead>
                      <tbody data-selectable>
                        {current.rows.slice(0, ROWS_SHOWN).map((r, i) => {
                          const values = built!.request.rows[i] ?? [];
                          const label = (key.length ? keyAt.map((at) => values[at]) : [values[0]]).map((v) => v ?? "NULL").join(", ");
                          return (
                            <tr key={i} className="[&>td]:border-b [&>td]:border-border [&>td]:px-2.5 [&>td]:py-1 [&>td]:align-top">
                              <td className="font-mono text-fg-subtle tabular-nums">{(built!.sourceRows[i] ?? i) + 1}</td>
                              <td>
                                <span className={cn("rounded px-1.5 py-px text-[10px] font-bold uppercase", TONE[r.status])}>
                                  {t(`smart.status.${r.status}`)}
                                </span>
                              </td>
                              <td className="max-w-56 truncate font-mono">{label}</td>
                              <td className="break-words text-fg-muted">
                                {r.message ?? (r.changed.length ? t("smart.changes", { columns: r.changed.join(", ") }) : "")}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                  {current.rows.length > ROWS_SHOWN && (
                    <p className="text-xs text-fg-subtle">{t("smart.rowsShown", { shown: ROWS_SHOWN, total: current.rows.length })}</p>
                  )}
                </>
              )}
            </section>
          )}

          {stale && !checking && <p className="text-xs text-warn">{t("smart.stale")}</p>}
          {error && (
            <div role="alert" data-smart-error className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
              <span data-selectable className="min-w-0 flex-1 break-words">
                {error}
              </span>
            </div>
          )}
        </div>
      )}

      <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
        {run ? (
          <>
            <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-sunken">
              <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${(run.done / Math.max(1, run.total)) * 100}%` }} />
            </div>
            <span className="shrink-0 text-xs text-fg-muted tabular-nums">{t("smart.running", { done: run.done, total: run.total })}</span>
            <Button variant="danger" size="sm" onClick={() => void ipc.cancelJob(run.job)}>
              {t("smart.stop")}
            </Button>
          </>
        ) : (
          <>
            {production && (
              <span className="flex items-center gap-1.5 text-xs font-medium text-danger">
                <TriangleAlert className="size-3.5" /> {t("review.prodWarning")}
              </span>
            )}
            <span className="flex-1" />
            <Button variant="ghost" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button loading={checking} disabled={!canPreview} onClick={() => void preview()}>
              {checking ? t("smart.previewing") : t("smart.preview")}
            </Button>
            <Button
              variant={production ? "danger" : "primary"}
              disabled={!current || blocked || toRun === 0}
              title={current ? undefined : t("smart.previewFirst")}
              onClick={() => void start()}
            >
              {production ? t("smart.runProd") : t("smart.run")}
              {current && toRun > 0 && !blocked && <span className="tabular-nums opacity-85">· {toRun}</span>}
            </Button>
          </>
        )}
      </div>
    </>
  );
}

function Check({ label, checked, disabled, onChange }: { label: string; checked: boolean; disabled?: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className={cn("flex items-center gap-1.5 text-[13px] text-fg-muted", disabled && "opacity-50")}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        className="size-3.5 accent-(--accent)"
      />
      {label}
    </label>
  );
}
