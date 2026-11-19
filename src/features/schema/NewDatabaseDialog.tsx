import { CircleAlert, DatabaseZap, Lock, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, Field, IconButton, Input, inputClass } from "../../components/ui/primitives";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ConnectionView, DatabaseOptions, NewDatabase } from "../../lib/types";
import { nodeKey, useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";

/**
 * "New database…" from a connection's menu. The form only chooses; the statement is built
 * by the backend, shown here as it will run, and run by the same code.
 */
export function NewDatabaseDialog() {
  const connection = useUi((s) => s.newDatabaseOn);
  const close = () => useUi.getState().askNewDatabase(null);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={connection !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-new-database
          // Start in the name field, not on the close button (which would pop its tooltip).
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).querySelector("input")?.focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[min(92vh,640px)] w-[min(94vw,560px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening, so the form starts empty and reads the server's options afresh. */}
          {connection && <Form key={connection.id} connection={connection} onBusy={setBusy} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface FormProps {
  connection: ConnectionView;
  onBusy: (busy: boolean) => void;
  onDone: () => void;
}

function Form({ connection, onBusy, onDone }: FormProps) {
  const { t } = useTranslation();
  const mysql = connection.engine === "mysql";
  const production = connection.environment === "production";

  const [options, setOptions] = useState<DatabaseOptions | null>(null);
  const [optionsError, setOptionsError] = useState<string | null>(null);
  const [name, setName] = useState("");
  // "" in a list means "leave it to the server", and is sent as no option at all.
  const [charset, setCharset] = useState("");
  const [collation, setCollation] = useState("");
  const [owner, setOwner] = useState("");
  const [template, setTemplate] = useState("");
  const [encoding, setEncoding] = useState("");
  // The backend's answer, with the choices it answers for: a statement may only be run
  // while it is the one on screen.
  const [plan, setPlan] = useState<({ sql: string } | { problem: string }) & { spec: NewDatabase } | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    ipc
      .databaseOptions(connection.id)
      .then((o) => !stale && setOptions(o))
      .catch((e: unknown) => !stale && setOptionsError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id]);

  const spec = useMemo<NewDatabase>(
    () =>
      mysql
        ? { name, charset: charset || null, collation: collation || null }
        : { name, owner: owner || null, template: template || null, encoding: encoding || null },
    [mysql, name, charset, collation, owner, template, encoding],
  );

  // The preview is the backend's own statement for exactly these choices.
  useEffect(() => {
    setError(null);
    if (name.trim() === "") return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planCreateDatabase(connection.id, spec)
        .then((sql) => !stale && setPlan({ spec, sql }))
        .catch((e: unknown) => !stale && setPlan({ spec, problem: errorMessage(e) }));
    }, 120);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, spec, name]);

  const collations = useMemo(() => {
    const of = charset || options?.defaultCharset;
    return options?.collations.filter((c) => c.charset === of) ?? [];
  }, [options, charset]);
  const charsetDefault = options?.charsets.find((c) => c.name === charset)?.defaultCollation;

  // Between a change and the backend's answer the statement shown is the previous one:
  // it is dimmed and cannot be run.
  const current = plan?.spec === spec;
  const ready = !!plan && current && "sql" in plan && !connection.readOnly && !creating;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setCreating(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.createDatabase(connection.id, spec);
      toast.success(t("schema.created", { name: name.trim() }));
      const store = useConnections.getState();
      store.toggle(nodeKey.connection(connection.id), true);
      await store.loadDatabases(connection.id);
      onBusy(false);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setCreating(false);
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-border py-3 pr-3 pl-5">
        <DatabaseZap className="size-5 shrink-0 text-accent-text" />
        <Dialog.Title className="min-w-0 flex-1 truncate text-[15px] font-semibold">
          {t("schema.newDatabaseTitle", { name: connection.name })}
        </Dialog.Title>
        <div className="flex h-7 shrink-0 items-center gap-1.5 rounded-lg border border-border px-1.5 text-xs">
          <EngineTag engine={connection.engine} />
          <EnvBadge environment={connection.environment} long />
        </div>
        <Dialog.Close asChild>
          <IconButton label={t("common.close")} disabled={creating}>
            <X className="size-4" />
          </IconButton>
        </Dialog.Close>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {connection.readOnly && (
          <div className="flex items-start gap-2 rounded-lg bg-sunken px-3 py-2 text-[13px] text-fg-muted">
            <Lock className="mt-0.5 size-4 shrink-0" /> {t("schema.readOnly")}
          </div>
        )}

        <Field label={t("schema.name")}>
          <Input autoFocus value={name} placeholder={t("schema.namePlaceholder")} disabled={creating} onChange={(e) => setName(e.target.value)} />
        </Field>

        {mysql ? (
          <div className="grid grid-cols-2 gap-3">
            <Field label={t("schema.charset")}>
              <select
                value={charset}
                disabled={creating || !options}
                onChange={(e) => {
                  setCharset(e.target.value);
                  setCollation(""); // a collation belongs to one character set
                }}
                className={inputClass}
              >
                <option value="">{t("schema.serverDefault", { value: options?.defaultCharset ?? "…" })}</option>
                {options?.charsets.map((c) => (
                  <option key={c.name} value={c.name} title={c.description}>
                    {c.name}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={t("schema.collation")}>
              <select value={collation} disabled={creating || !options} onChange={(e) => setCollation(e.target.value)} className={inputClass}>
                <option value="">
                  {charset
                    ? t("schema.charsetDefault", { value: charsetDefault ?? "…" })
                    : t("schema.serverDefault", { value: options?.defaultCollation ?? "…" })}
                </option>
                {collations.map((c) => (
                  <option key={c.name} value={c.name}>
                    {c.name}
                  </option>
                ))}
              </select>
            </Field>
          </div>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-3">
              <Field label={t("schema.owner")}>
                <select value={owner} disabled={creating || !options} onChange={(e) => setOwner(e.target.value)} className={inputClass}>
                  <option value="">{t("schema.ownerDefault", { value: options?.currentUser ?? "…" })}</option>
                  {options?.owners
                    .filter((o) => o !== options.currentUser)
                    .map((o) => (
                      <option key={o} value={o}>
                        {o}
                      </option>
                    ))}
                </select>
              </Field>
              <Field label={t("schema.template")}>
                <select value={template} disabled={creating || !options} onChange={(e) => setTemplate(e.target.value)} className={inputClass}>
                  <option value="">{t("schema.templateDefault")}</option>
                  {options?.templates
                    .filter((x) => x !== "template1")
                    .map((x) => (
                      <option key={x} value={x}>
                        {x}
                      </option>
                    ))}
                </select>
              </Field>
            </div>
            <Field label={t("schema.encoding")} hint={encoding && template !== "template0" ? t("schema.templateHint") : undefined}>
              <select value={encoding} disabled={creating || !options} onChange={(e) => setEncoding(e.target.value)} className={inputClass}>
                <option value="">{t("schema.encodingDefault", { value: options?.defaultEncoding ?? "…" })}</option>
                {options?.encodings.map((x) => (
                  <option key={x} value={x}>
                    {x}
                  </option>
                ))}
              </select>
            </Field>
          </>
        )}

        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium text-fg-muted">{t("schema.statement")}</span>
          {plan && "sql" in plan ? (
            <pre data-selectable data-ddl-preview aria-busy={!current} className={`${current ? "" : "opacity-50 "}rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap transition-opacity`}>
              {plan.sql}
            </pre>
          ) : (
            <div data-ddl-preview-empty className="rounded-lg border border-dashed border-border px-3 py-2 text-xs text-fg-subtle">
              {plan ? <span className="text-danger">{plan.problem}</span> : t("schema.statementEmpty")}
            </div>
          )}
        </div>

        {optionsError && <p className="text-xs text-warn">{t("schema.optionsFailed", { message: optionsError })}</p>}
        {production && !connection.readOnly && (
          <div className="flex items-center gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] font-medium text-danger">
            <TriangleAlert className="size-4 shrink-0" /> {t("schema.prodWarning")}
          </div>
        )}
        {error && (
          <div role="alert" data-ddl-error className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
            <span data-selectable className="min-w-0 flex-1 break-words">
              {error}
            </span>
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3">
        <Dialog.Close asChild>
          <Button variant="ghost" disabled={creating}>
            {t("common.cancel")}
          </Button>
        </Dialog.Close>
        <Button type="submit" variant={production ? "danger" : "primary"} loading={creating} disabled={!ready}>
          {creating ? t("schema.creating") : production ? t("schema.createProd") : t("schema.create")}
        </Button>
      </div>
    </form>
  );
}
