import { CircleCheck, Copy, GitCompareArrows } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, inputClass, Spinner } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { Comparison, ConnectionView } from "../../lib/types";
import { sessionOf, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import { describeColumn, describeIndex, describeKey, reportText, tally } from "./compareText";
import { Failure, Header } from "./parts";

/**
 * "Compare structure…": this database (on PostgreSQL, this schema) as A, another of the same
 * engine as B — on this connection or another, local and production say. Reads only. The
 * report: tables on one side only, and for the tables both have, the columns, indexes and
 * foreign keys that differ.
 */
export function CompareDialog() {
  const side = useUi((s) => s.compareOf);
  const close = () => useUi.getState().askCompare(null);
  return (
    <Dialog.Root open={side !== null} onOpenChange={(open) => !open && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-compare
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(90vh,820px)] w-[min(95vw,1000px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {side && <Comparer key={`${side.connection.id}/${side.database}/${side.schema ?? ""}`} {...side} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Comparer({ connection, database, schema }: { connection: ConnectionView; database: string; schema: string | null }) {
  const { t } = useTranslation();
  const connections = useConnections((s) => s.connections).filter((c) => c.engine === connection.engine);
  const sessions = useConnections((s) => s.sessions);
  const [otherId, setOtherId] = useState(connection.id);
  const [otherDb, setOtherDb] = useState("");
  const [otherSchema, setOtherSchema] = useState<string | null>(null);
  const [databases, setDatabases] = useState<string[] | null>(null);
  const [schemas, setSchemas] = useState<string[] | null>(null);
  const [result, setResult] = useState<{ of: string; comparison: Comparison } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const postgres = connection.engine === "postgres";
  const other = connections.find((c) => c.id === otherId);
  const otherSession = sessionOf(sessions, otherId);
  const aLabel = [connection.name, database, schema].filter(Boolean).join(" › ");
  const bLabel = [other?.name, otherDb, otherSchema].filter(Boolean).join(" › ");
  const ready = !!otherDb && (!postgres || !!otherSchema);
  const sameSide = otherId === connection.id && otherDb === database && (otherSchema ?? null) === schema;
  const asked = JSON.stringify([otherId, otherDb, otherSchema]);
  const current = result?.of === asked ? result.comparison : null;

  useEffect(() => {
    if (otherSession.status === "idle" || otherSession.status === "error") void useConnections.getState().connect(otherId);
  }, [otherId, otherSession.status]);
  useEffect(() => {
    if (otherSession.status !== "connected") return;
    let stale = false;
    setDatabases(null);
    ipc
      .listDatabases(otherId)
      .then((l) => !stale && setDatabases(l.filter((d) => !d.system).map((d) => d.name)))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [otherId, otherSession.status]);
  useEffect(() => {
    if (!postgres || !otherDb || otherSession.status !== "connected") return;
    let stale = false;
    setSchemas(null);
    ipc
      .listSchemas(otherId, otherDb)
      .then((l) => !stale && setSchemas(l.filter((s) => !s.system).map((s) => s.name)))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [postgres, otherId, otherDb, otherSession.status]);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const comparison = await ipc.compareStructure(connection.id, database, schema, otherId, otherDb, otherSchema);
      setResult({ of: asked, comparison });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const copy = async () => {
    if (!current) return;
    try {
      await clipboard.write(reportText(current, aLabel, bLabel));
      toast.success(t("compare.copied"), { id: "compare-copied", duration: 2000 });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const select = cn(inputClass, "h-8 text-[13px]");
  const label = "flex flex-col gap-1 text-[11px] text-fg-muted";
  const n = current && tally(current);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Header icon={<GitCompareArrows className="size-5 shrink-0 text-accent-text" />} title={t("compare.title")} connection={connection} busy={busy} />
      <div className="flex shrink-0 flex-col gap-3 border-b border-border px-5 py-4">
        <div className="flex items-center gap-2 text-[13px]">
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-accent-soft text-xs font-bold text-accent-text">A</span>
          <span data-compare-a className="truncate font-medium">
            {aLabel}
          </span>
        </div>
        <div className="flex items-end gap-2">
          <span className="mb-1 flex size-6 shrink-0 items-center justify-center rounded-md bg-sunken text-xs font-bold text-fg-muted">B</span>
          <div className="grid flex-1 grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-2">
            <label className={label}>
              {t("copy.connection")}
              <select
                data-compare-connection
                value={otherId}
                onChange={(e) => {
                  setOtherId(e.target.value);
                  setOtherDb("");
                  setOtherSchema(null);
                }}
                className={select}
              >
                {connections.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            </label>
            <label className={label}>
              {t("copy.database")}
              <select
                data-compare-database
                value={otherDb}
                disabled={!databases}
                onChange={(e) => {
                  setOtherDb(e.target.value);
                  setOtherSchema(null);
                }}
                className={select}
              >
                <option value="">{databases ? t("copy.pick") : t("common.loading")}</option>
                {databases?.map((d) => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </select>
            </label>
            {postgres && (
              <label className={label}>
                {t("copy.schema")}
                <select
                  data-compare-schema
                  value={otherSchema ?? ""}
                  disabled={!schemas}
                  onChange={(e) => setOtherSchema(e.target.value || null)}
                  className={select}
                >
                  <option value="">{schemas ? t("copy.pick") : t("common.loading")}</option>
                  {schemas?.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </div>
          <Button variant="primary" data-compare-run loading={busy} disabled={!ready || sameSide || busy} onClick={() => void run()}>
            {t("compare.run")}
          </Button>
        </div>
        {sameSide && <p className="text-xs text-fg-subtle">{t("compare.sameSide")}</p>}
        {otherSession.status === "connecting" && (
          <p className="flex items-center gap-2 text-xs text-fg-muted">
            <Spinner className="size-3" /> {t("copy.connecting")}
          </p>
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        <Failure message={error} />
        {!current && !error && <p className="py-10 text-center text-[13px] text-fg-subtle">{t("compare.nothingYet")}</p>}
        {current && n && (
          <>
            <div data-compare-summary className="flex flex-wrap items-center gap-x-5 gap-y-1 text-[13px]">
              {n.identical ? (
                <span className="flex items-center gap-2 font-medium text-ok">
                  <CircleCheck className="size-4" /> {t("compare.identical", { count: n.same })}
                </span>
              ) : (
                (
                  [
                    ["changed", n.changed, "text-warn"],
                    ["onlyA", n.onlyA, "text-accent-text"],
                    ["onlyB", n.onlyB, "text-info"],
                    ["same", n.same, "text-fg-muted"],
                  ] as const
                ).map(([k, count, tone]) => (
                  <span key={k} data-compare-count={k} className={cn(count === 0 && "opacity-45")}>
                    <b className={cn("text-[15px] tabular-nums", tone)}>{count}</b> <span className="text-fg-muted">{t(`compare.count.${k}`)}</span>
                  </span>
                ))
              )}
            </div>

            {(current.onlyA.length > 0 || current.viewsOnlyA.length > 0) && (
              <Names title={t("compare.onlyA")} tables={current.onlyA} views={current.viewsOnlyA} data="a" />
            )}
            {(current.onlyB.length > 0 || current.viewsOnlyB.length > 0) && (
              <Names title={t("compare.onlyB")} tables={current.onlyB} views={current.viewsOnlyB} data="b" />
            )}

            {current.changed.map((table) => (
              <section key={table.name} data-compare-table={table.name} className="overflow-hidden rounded-xl border border-border">
                <h3 className="border-b border-border bg-sunken/60 px-3 py-2 text-[13px] font-semibold">{table.name}</h3>
                <table className="w-full border-separate border-spacing-0 text-xs">
                  <thead>
                    <tr className="[&>th]:border-b [&>th]:border-border [&>th]:px-3 [&>th]:py-1.5 [&>th]:text-left [&>th]:font-semibold [&>th]:text-fg-muted">
                      <th className="w-44"></th>
                      <th>A</th>
                      <th>B</th>
                    </tr>
                  </thead>
                  <tbody data-selectable>
                    {table.columns.map((c) => (
                      <tr
                        key={`c:${c.name}`}
                        data-compare-column={c.name}
                        className="[&>td]:border-b [&>td]:border-border [&>td]:px-3 [&>td]:py-1.5 [&>td]:align-top"
                      >
                        <td className="font-medium">
                          {c.name}
                          {c.fields.length > 0 && (
                            <span className="ml-1.5 text-[11px] font-normal text-warn">{c.fields.map((f) => t(`compare.field.${f}`)).join(", ")}</span>
                          )}
                        </td>
                        <Cell text={c.a && describeColumn(c.a)} />
                        <Cell text={c.b && describeColumn(c.b)} />
                      </tr>
                    ))}
                    {table.indexes.map((i, at) => (
                      <tr key={`i:${at}`} data-compare-index className="[&>td]:border-b [&>td]:border-border [&>td]:px-3 [&>td]:py-1.5 [&>td]:align-top">
                        <td className="text-fg-muted">{t("compare.index")}</td>
                        <Cell text={i.a && describeIndex(i.a)} />
                        <Cell text={i.b && describeIndex(i.b)} />
                      </tr>
                    ))}
                    {table.foreignKeys.map((k, at) => (
                      <tr key={`k:${at}`} data-compare-key className="[&>td]:border-b [&>td]:border-border [&>td]:px-3 [&>td]:py-1.5 [&>td]:align-top">
                        <td className="text-fg-muted">{t("compare.foreignKey")}</td>
                        <Cell text={k.a && describeKey(k.a)} />
                        <Cell text={k.b && describeKey(k.b)} />
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            ))}
            <p className="text-xs leading-relaxed text-fg-subtle">{t("compare.how")}</p>
          </>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
        <span className="flex-1" />
        <Button size="sm" data-compare-copy icon={<Copy className="size-3.5" />} disabled={!current} onClick={() => void copy()}>
          {t("compare.copy")}
        </Button>
        <Dialog.Close asChild>
          <Button variant="ghost">{t("common.close")}</Button>
        </Dialog.Close>
      </div>
    </div>
  );
}

function Cell({ text }: { text: string | null }) {
  const { t } = useTranslation();
  return <td className={cn("font-mono text-[11px] break-words", text ? "text-fg" : "text-fg-subtle italic")}>{text ?? t("compare.absent")}</td>;
}

function Names({ title, tables, views, data }: { title: string; tables: string[]; views: string[]; data: string }) {
  const { t } = useTranslation();
  return (
    <section data-compare-only={data} className="flex flex-col gap-1.5">
      <h3 className="text-[11px] font-semibold tracking-wider text-fg-subtle uppercase">{title}</h3>
      <div className="flex flex-wrap gap-1.5">
        {tables.map((n) => (
          <span key={`t:${n}`} className="rounded-md bg-sunken px-2 py-0.5 font-mono text-xs">
            {n}
          </span>
        ))}
        {views.map((n) => (
          <span key={`v:${n}`} className="rounded-md bg-sunken px-2 py-0.5 font-mono text-xs text-fg-muted">
            {n} <span className="font-sans text-[10px]">{t("compare.view")}</span>
          </span>
        ))}
      </div>
    </section>
  );
}
