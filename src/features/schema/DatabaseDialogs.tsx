import { DatabaseBackup, Lock, RotateCw, Trash2, TriangleAlert } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, Field, Input, inputClass, Spinner } from "../../components/ui/primitives";
import { errorMessage, ipc } from "../../lib/ipc";
import type { AlterDatabase, DatabaseDetails, DatabaseOptions } from "../../lib/types";
import { useConnections } from "../../stores/connections";
import { type DatabaseTask, useUi } from "../../stores/ui";
import { useWorkspace } from "../../stores/workspace";
import { Failure, Footer, Header, Notice, type Plan, Statement } from "./parts";

/**
 * What a database's or a connection's menu does to the server: edit a database, drop one,
 * flush. As in "New database…", the form only chooses; every statement is built by the
 * backend, shown here as it will run, and run by the same code.
 */
export function DatabaseDialogs() {
  const task = useUi((s) => s.databaseTask);
  const close = () => useUi.getState().askDatabaseTask(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={task !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-database-dialog={task?.kind}
          // Start in the first field, not on the close button (which would pop its tooltip).
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const root = e.currentTarget as HTMLElement;
            (root.querySelector<HTMLElement>("input:not([disabled]), select:not([disabled])") ?? root).focus();
          }}
          tabIndex={-1}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[min(92vh,640px)] w-[min(94vw,540px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening, so each form reads the server afresh. */}
          {task?.kind === "edit" && <EditForm task={task} onBusy={setBusy} onDone={close} />}
          {task?.kind === "drop" && <DropForm task={task} onBusy={setBusy} onDone={close} />}
          {task?.kind === "flush" && <FlushForm task={task} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface FormProps<K extends DatabaseTask["kind"]> {
  task: Extract<DatabaseTask, { kind: K }>;
  onBusy: (busy: boolean) => void;
  onDone: () => void;
}

/** Tabs of a database that goes away or gets another name follow, and the explorer reloads. */
async function afterDatabaseChanged(connectionId: string, database: string, renamedTo: string | null) {
  useWorkspace.getState().databaseGone(connectionId, database, renamedTo);
  const store = useConnections.getState();
  store.forgetDatabase(connectionId, database);
  await store.loadDatabases(connectionId);
}

// ---- Edit --------------------------------------------------------------------------

function EditForm({ task, onBusy, onDone }: FormProps<"edit">) {
  const { t } = useTranslation();
  const { connection, database } = task;
  const mysql = connection.engine === "mysql";
  const production = connection.environment === "production";

  const [details, setDetails] = useState<DatabaseDetails | null>(null);
  const [options, setOptions] = useState<DatabaseOptions | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [charset, setCharset] = useState("");
  const [collation, setCollation] = useState("");
  const [newName, setNewName] = useState(database);
  const [owner, setOwner] = useState("");
  const [plan, setPlan] = useState<(NonNullable<Plan> & { spec: AlterDatabase }) | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    Promise.all([ipc.databaseDetails(connection.id, database), ipc.databaseOptions(connection.id)])
      .then(([d, o]) => {
        if (stale) return;
        setDetails(d);
        setOptions(o);
        setCharset(d.charset ?? "");
        setCollation(d.collation ?? "");
        setOwner(d.owner ?? "");
      })
      .catch((e: unknown) => !stale && setLoadError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, database]);

  // Only what differs from the database as it is now is sent, so the statement says
  // exactly what changes. `null`: nothing differs yet.
  const spec = useMemo<AlterDatabase | null>(() => {
    if (!details) return null;
    const s: AlterDatabase = { name: database };
    if (mysql) {
      if (charset && charset !== details.charset) s.charset = charset;
      if (collation && collation !== details.collation) s.collation = collation;
    } else {
      if (newName.trim() && newName.trim() !== database) s.newName = newName.trim();
      if (owner && owner !== details.owner) s.owner = owner;
    }
    return Object.keys(s).length > 1 ? s : null;
  }, [details, database, mysql, charset, collation, newName, owner]);

  useEffect(() => {
    setError(null);
    if (!spec) return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planAlterDatabase(connection.id, spec)
        .then((statements) => !stale && setPlan({ spec, sql: statements.map((s) => `${s};`).join("\n") }))
        .catch((e: unknown) => !stale && setPlan({ spec, problem: errorMessage(e) }));
    }, 120);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, spec]);

  const sameCharset = charset === (details?.charset ?? "");
  const collations = useMemo(() => options?.collations.filter((c) => c.charset === charset) ?? [], [options, charset]);
  const charsetDefault = options?.charsets.find((c) => c.name === charset)?.defaultCollation;
  // A new name moves the database out from under its open tabs.
  const blocked = useWorkspace((s) => !!spec?.newName && s.hasUnsavedInDatabase(connection.id, database));

  const current = plan?.spec === spec;
  const ready = !!plan && current && "sql" in plan && !connection.readOnly && !blocked && !saving;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready || !spec) return;
    setSaving(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.alterDatabase(connection.id, spec);
      toast.success(t("schema.altered", { name: spec.newName ?? database }));
      if (spec.newName) await afterDatabaseChanged(connection.id, database, spec.newName);
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <Header
        icon={<DatabaseBackup className="size-5 shrink-0 text-accent-text" />}
        title={t("schema.editDatabaseTitle", { name: database })}
        connection={connection}
        busy={saving}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyChange")}
          </Notice>
        )}
        {loadError ? (
          <Failure message={loadError} />
        ) : !details ? (
          <div className="flex items-center gap-2 py-6 text-[13px] text-fg-muted">
            <Spinner className="size-4" /> {t("common.loading")}
          </div>
        ) : mysql ? (
          <>
            <Field label={t("schema.name")} hint={t("schema.renameUnsupported")}>
              <Input value={database} disabled readOnly />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label={t("schema.charset")}>
                <select
                  value={charset}
                  disabled={saving}
                  onChange={(e) => {
                    const next = e.target.value;
                    setCharset(next);
                    // A collation belongs to one character set.
                    setCollation(next === details.charset ? (details.collation ?? "") : "");
                  }}
                  className={inputClass}
                >
                  {options?.charsets.map((c) => (
                    <option key={c.name} value={c.name} title={c.description}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={t("schema.collation")}>
                <select value={collation} disabled={saving} onChange={(e) => setCollation(e.target.value)} className={inputClass}>
                  {!sameCharset && <option value="">{t("schema.charsetDefault", { value: charsetDefault ?? "…" })}</option>}
                  {collations.map((c) => (
                    <option key={c.name} value={c.name}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            <p className="-mt-2 text-xs text-fg-subtle">{t("schema.charsetScope")}</p>
          </>
        ) : (
          <>
            <Field label={t("schema.name")}>
              <Input value={newName} disabled={saving} onChange={(e) => setNewName(e.target.value)} />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label={t("schema.owner")}>
                <select value={owner} disabled={saving} onChange={(e) => setOwner(e.target.value)} className={inputClass}>
                  {/* The owner may be a role the list leaves out. */}
                  {details.owner && !options?.owners.includes(details.owner) && <option value={details.owner}>{details.owner}</option>}
                  {options?.owners.map((o) => (
                    <option key={o} value={o}>
                      {o}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={t("schema.encoding")} hint={t("schema.encodingFixed")}>
                <Input value={details.encoding ?? ""} disabled readOnly />
              </Field>
            </div>
          </>
        )}

        {details && <Statement plan={plan} empty={t("schema.nothingChanged")} stale={!current} />}
        {blocked && (
          <Notice tone="warn" icon={<TriangleAlert className="size-4" />}>
            {t("schema.unsavedTabs")}
          </Notice>
        )}
        {production && !connection.readOnly && details && (
          <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
            {t("schema.prodChangeWarning")}
          </Notice>
        )}
        <Failure message={error} />
      </div>
      <Footer busy={saving}>
        <Button type="submit" variant={production ? "danger" : "primary"} loading={saving} disabled={!ready}>
          {saving ? t("schema.saving") : t("schema.saveChanges")}
        </Button>
      </Footer>
    </form>
  );
}

// ---- Drop --------------------------------------------------------------------------

function DropForm({ task, onBusy, onDone }: FormProps<"drop">) {
  const { t } = useTranslation();
  const { connection, database } = task;
  const production = connection.environment === "production";

  const [details, setDetails] = useState<DatabaseDetails | null>(null);
  const [plan, setPlan] = useState<Plan>(null);
  const [typed, setTyped] = useState("");
  const [dropping, setDropping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const blocked = useWorkspace((s) => s.hasUnsavedInDatabase(connection.id, database));
  const openTabs = useWorkspace(
    (s) => s.tabs.filter((x) => x.kind === "table" && x.connectionId === connection.id && x.table.database === database).length,
  );

  useEffect(() => {
    let stale = false;
    ipc
      .planDropDatabase(connection.id, database)
      .then((sql) => !stale && setPlan({ sql }))
      .catch((e: unknown) => !stale && setPlan({ problem: errorMessage(e) }));
    // What is about to be lost. The form works without it.
    ipc
      .databaseDetails(connection.id, database)
      .then((d) => !stale && setDetails(d))
      .catch(() => undefined);
    return () => {
      stale = true;
    };
  }, [connection.id, database]);

  const ready = !!plan && "sql" in plan && typed === database && !connection.readOnly && !blocked && !dropping;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setDropping(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.dropDatabase(connection.id, database, typed);
      toast.success(t("schema.dropped", { name: database }));
      await afterDatabaseChanged(connection.id, database, null);
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setDropping(false);
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <Header
        icon={<Trash2 className="size-5 shrink-0 text-danger" />}
        title={t("schema.dropDatabaseTitle", { name: database })}
        connection={connection}
        busy={dropping}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyChange")}
          </Notice>
        )}
        <div className="flex flex-col gap-1 text-[13px] leading-relaxed">
          <p>{t("schema.dropWarning")}</p>
          <p data-drop-contents className="text-fg-muted">
            {details?.tables == null
              ? null
              : details.tables === 0
                ? t("schema.dropHoldsNothing")
                : t("schema.dropHolds", { count: details.tables })}
            {openTabs > 0 && ` ${t("schema.dropClosesTabs", { count: openTabs })}`}
          </p>
        </div>

        <Statement plan={plan} empty="…" />

        <Field label={t("schema.dropType", { name: database })}>
          <Input
            value={typed}
            disabled={dropping || connection.readOnly || !plan || "problem" in plan}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            placeholder={database}
            onChange={(e) => setTyped(e.target.value)}
          />
        </Field>

        {blocked && (
          <Notice tone="warn" icon={<TriangleAlert className="size-4" />}>
            {t("schema.unsavedTabs")}
          </Notice>
        )}
        {production && !connection.readOnly && (
          <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
            {t("schema.prodDropWarning")}
          </Notice>
        )}
        <Failure message={error} />
      </div>
      <Footer busy={dropping}>
        <Button type="submit" variant="danger" loading={dropping} disabled={!ready}>
          {dropping ? t("schema.dropping") : production ? t("schema.dropProd") : t("schema.drop")}
        </Button>
      </Footer>
    </form>
  );
}

// ---- Flush (MySQL) -----------------------------------------------------------------

function FlushForm({ task, onBusy, onDone }: FormProps<"flush">) {
  const { t } = useTranslation();
  const { connection, what } = task;
  const production = connection.environment === "production";
  const [plan, setPlan] = useState<Plan>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    ipc
      .planFlush(connection.id, what)
      .then((sql) => !stale && setPlan({ sql }))
      .catch((e: unknown) => !stale && setPlan({ problem: errorMessage(e) }));
    return () => {
      stale = true;
    };
  }, [connection.id, what]);

  const ready = !!plan && "sql" in plan && !connection.readOnly && !running;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setRunning(true);
    onBusy(true);
    setError(null);
    try {
      const sql = await ipc.flushServer(connection.id, what);
      toast.success(t("schema.flushed", { statement: sql }));
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setRunning(false);
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <Header
        icon={<RotateCw className="size-5 shrink-0 text-accent-text" />}
        title={t("schema.flushTitle", { what: t(`schema.flushKinds.${what}`) })}
        connection={connection}
        busy={running}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <Notice tone="muted" icon={<Lock className="size-4" />}>
            {t("schema.readOnlyChange")}
          </Notice>
        )}
        <p className="text-[13px] leading-relaxed">{t(`schema.flushAbout.${what}`)}</p>
        <Statement plan={plan} empty="…" />
        {production && !connection.readOnly && (
          <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
            {t("schema.prodFlushWarning")}
          </Notice>
        )}
        <Failure message={error} />
      </div>
      <Footer busy={running}>
        <Button type="submit" variant={production ? "danger" : "primary"} loading={running} disabled={!ready}>
          {running ? t("schema.running") : t("schema.run")}
        </Button>
      </Footer>
    </form>
  );
}
