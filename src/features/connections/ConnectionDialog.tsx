import { CircleAlert, CircleCheck, Eye, EyeOff, KeyRound, Lock, LockOpen, Star, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { EngineTag } from "../../components/Brand";
import { Button, Field, IconButton, Input, inputClass, Segmented, Switch } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ConnectionProfile, ConnectionView, Engine, Environment, PasswordChange, ServerInfo, TlsMode } from "../../lib/types";
import { useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";

const DEFAULT_PORT: Record<Engine, number> = { mysql: 3306, postgres: 5432, sqlserver: 1433 };
const ENGINES: { value: Engine; available: boolean }[] = [
  { value: "mysql", available: true },
  { value: "postgres", available: true },
  { value: "sqlserver", available: false },
];
const ENVIRONMENTS: { value: Environment; selected: string }[] = [
  { value: "development", selected: "text-ok" },
  { value: "staging", selected: "text-info" },
  { value: "production", selected: "text-danger" },
];
const TLS_MODES: TlsMode[] = ["disable", "prefer", "require", "verify-ca", "verify-full"];

function blank(): ConnectionProfile {
  return {
    id: "",
    name: "",
    engine: "mysql",
    environment: "development",
    host: "localhost",
    port: DEFAULT_PORT.mysql,
    username: "",
    database: null,
    tls: "prefer",
    tlsCaPath: null,
    readOnly: false,
    favorite: false,
    tags: [],
    connectTimeoutSecs: 10,
    bulkReviewRows: 10,
    createdAt: 0,
    updatedAt: 0,
    lastUsedAt: null,
  };
}

type TestState = { status: "idle" } | { status: "running" } | { status: "ok"; info: ServerInfo } | { status: "error"; message: string };

export function ConnectionDialog() {
  const target = useUi((s) => s.connectionDialog);
  const close = useUi((s) => s.closeConnectionDialog);
  return (
    <Dialog.Root open={target !== null} onOpenChange={(open) => !open && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[min(92vh,760px)] w-[min(94vw,620px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Remount per target so the form always starts from that connection's values. */}
          {target !== null && <Form key={target === "new" ? "new" : target.id} existing={target === "new" ? null : target} onDone={close} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Form({ existing, onDone }: { existing: ConnectionView | null; onDone: () => void }) {
  const { t } = useTranslation();
  const [form, setForm] = useState<ConnectionProfile>(() => {
    if (!existing) return blank();
    const { hasPassword: _hasPassword, ...profile } = existing;
    return profile;
  });
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [tags, setTags] = useState(form.tags.join(", "));
  const [port, setPort] = useState(String(form.port));
  const [test, setTest] = useState<TestState>({ status: "idle" });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const patch = (p: Partial<ConnectionProfile>) => {
    setForm((f) => ({ ...f, ...p }));
    setTest({ status: "idle" }); // a result for other settings would be misleading
  };

  const portNumber = Number(port);
  const portValid = /^\d+$/.test(port) && portNumber >= 1 && portNumber <= 65535;
  const complete = form.name.trim() !== "" && form.host.trim() !== "" && form.username.trim() !== "" && portValid;

  const profile = (): ConnectionProfile => ({
    ...form,
    port: portNumber,
    tags: tags.split(",").map((s) => s.trim()).filter(Boolean),
  });
  const passwordChange = (): PasswordChange =>
    clearPassword ? { action: "clear" } : password !== "" ? { action: "set", value: password } : { action: "keep" };

  const runTest = async () => {
    setTest({ status: "running" });
    try {
      setTest({ status: "ok", info: await ipc.testConnection(profile(), passwordChange()) });
    } catch (e) {
      setTest({ status: "error", message: errorMessage(e) });
    }
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!complete || saving) return;
    setSaving(true);
    setError(null);
    try {
      await useConnections.getState().save(profile(), passwordChange());
      toast.success(t("connection.saved"));
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
    }
  };

  const usesCa = form.tls === "verify-ca" || form.tls === "verify-full";

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-border py-3 pr-3 pl-5">
        <Dialog.Title className="flex-1 text-[15px] font-semibold">
          {existing ? t("connection.editTitle") : t("connection.newTitle")}
        </Dialog.Title>
        <IconButton label={form.favorite ? t("explorer.unfavorite") : t("explorer.favorite")} active={form.favorite} onClick={() => patch({ favorite: !form.favorite })}>
          <Star className={cn("size-4", form.favorite && "fill-current")} />
        </IconButton>
        <Dialog.Close asChild>
          <IconButton label={t("common.close")}>
            <X className="size-4" />
          </IconButton>
        </Dialog.Close>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs font-medium text-fg-muted">{t("connection.engine")}</span>
          <div role="radiogroup" aria-label={t("connection.engine")} className="grid grid-cols-3 gap-2">
            {ENGINES.map(({ value, available }) => {
              const selected = form.engine === value;
              return (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  disabled={!available}
                  title={available ? undefined : t("connection.sqlserverNote")}
                  onClick={() => {
                    // Follow the engine's default port unless the user typed their own.
                    if (port === String(DEFAULT_PORT[form.engine])) setPort(String(DEFAULT_PORT[value]));
                    patch({ engine: value });
                  }}
                  className={cn(
                    "flex h-11 items-center gap-2 rounded-xl border px-3 text-left text-[13px] transition-colors",
                    selected ? "border-accent bg-accent-soft font-semibold ring-1 ring-accent" : "border-border-strong hover:bg-sunken",
                    !available && "cursor-not-allowed opacity-55 hover:bg-transparent",
                  )}
                >
                  <EngineTag engine={value} className="size-6 text-[11px]" />
                  <span className="min-w-0 flex-1 truncate">{t(`engine.${value}`)}</span>
                  {!available && (
                    <span className="rounded bg-sunken px-1.5 py-0.5 text-[10px] font-semibold text-fg-muted">{t("common.planned")}</span>
                  )}
                </button>
              );
            })}
          </div>
        </div>

        <div className="grid grid-cols-[1fr_auto] items-end gap-3">
          <Field label={t("connection.name")}>
            <Input autoFocus value={form.name} placeholder={t("connection.namePlaceholder")} onChange={(e) => patch({ name: e.target.value })} />
          </Field>
          <div className="flex flex-col gap-1">
            <span className="text-xs font-medium text-fg-muted">{t("connection.environment")}</span>
            <Segmented
              label={t("connection.environment")}
              value={form.environment}
              onChange={(environment) => patch({ environment })}
              options={ENVIRONMENTS.map((e) => ({ value: e.value, label: t(`env.${e.value}`), className: e.selected }))}
            />
          </div>
        </div>

        <div className="grid grid-cols-[1fr_96px] gap-3">
          <Field label={t("connection.host")}>
            <Input value={form.host} placeholder="localhost" onChange={(e) => patch({ host: e.target.value })} />
          </Field>
          <Field label={t("connection.port")}>
            <Input
              value={port}
              inputMode="numeric"
              aria-invalid={!portValid}
              className={cn("tabular-nums", !portValid && "border-danger focus:border-danger")}
              onChange={(e) => {
                setPort(e.target.value.trim());
                setTest({ status: "idle" });
              }}
            />
          </Field>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <Field label={t("connection.username")}>
            <Input value={form.username} onChange={(e) => patch({ username: e.target.value })} />
          </Field>
          <Field label={t("connection.database")} optional={t("common.optional")}>
            <Input
              value={form.database ?? ""}
              placeholder={t(form.engine === "postgres" ? "connection.databasePlaceholderPg" : "connection.databasePlaceholderMy")}
              onChange={(e) => patch({ database: e.target.value || null })}
            />
          </Field>
        </div>

        <Field
          label={t("connection.password")}
          hint={
            clearPassword ? (
              <span className="text-warn">
                {t("connection.passwordWillClear")}{" "}
                <button type="button" className="font-medium underline underline-offset-2" onClick={() => setClearPassword(false)}>
                  {t("connection.undo")}
                </button>
              </span>
            ) : existing?.hasPassword ? (
              <span className="inline-flex flex-wrap items-center gap-x-1">
                <KeyRound className="size-3 shrink-0" /> {t("connection.passwordSaved")}
                {password === "" && (
                  <button type="button" className="font-medium text-fg-muted underline underline-offset-2" onClick={() => setClearPassword(true)}>
                    {t("connection.passwordClear")}
                  </button>
                )}
              </span>
            ) : (
              t("connection.passwordHint")
            )
          }
        >
          <div className="relative">
            <Input
              type={showPassword ? "text" : "password"}
              value={password}
              disabled={clearPassword}
              placeholder={existing?.hasPassword && !clearPassword ? "••••••••" : undefined}
              className="pr-9"
              onChange={(e) => {
                setPassword(e.target.value);
                setTest({ status: "idle" });
              }}
            />
            <button
              type="button"
              aria-label={showPassword ? t("connection.hide") : t("connection.show")}
              onClick={() => setShowPassword((v) => !v)}
              className="absolute top-1 right-1 flex size-6 items-center justify-center rounded-md text-fg-subtle hover:bg-sunken hover:text-fg"
            >
              {showPassword ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
            </button>
          </div>
        </Field>

        <fieldset className="flex flex-col gap-3 rounded-xl border border-border p-3.5">
          <legend className="px-1 text-xs font-semibold text-fg-muted">{t("connection.security")}</legend>
          <Field label={t("connection.tls")}>
            <select value={form.tls} onChange={(e) => patch({ tls: e.target.value as TlsMode })} className={inputClass}>
              {TLS_MODES.map((m) => (
                <option key={m} value={m}>
                  {t(`connection.tlsMode.${m}`)}
                </option>
              ))}
            </select>
          </Field>
          {usesCa && (
            <Field label={t("connection.tlsCa")} optional={t("common.optional")}>
              <Input
                value={form.tlsCaPath ?? ""}
                placeholder={t("connection.tlsCaPlaceholder")}
                className="font-mono text-xs"
                onChange={(e) => patch({ tlsCaPath: e.target.value || null })}
              />
            </Field>
          )}
          <div className="flex items-center gap-3">
            <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-sunken text-fg-muted">
              {form.readOnly ? <Lock className="size-4" /> : <LockOpen className="size-4" />}
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium">{t("connection.readOnly")}</div>
              <div className="text-xs leading-snug text-fg-subtle">{t("connection.readOnlyHint")}</div>
            </div>
            <Switch checked={form.readOnly} onChange={(readOnly) => patch({ readOnly })} label={t("connection.readOnly")} />
          </div>
          <div className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium">{t("connection.reviewRows")}</div>
              <div className="text-xs leading-snug text-fg-subtle">{t("connection.reviewRowsHint")}</div>
            </div>
            {/* The input is as wide as its box: the width belongs on the box. */}
            <div className="w-20 shrink-0">
              <Input
                value={String(form.bulkReviewRows)}
                inputMode="numeric"
                aria-label={t("connection.reviewRows")}
                className="text-right tabular-nums"
                onChange={(e) => {
                  const n = Number(e.target.value.replace(/\D/g, "").slice(0, 5));
                  setForm((f) => ({ ...f, bulkReviewRows: n }));
                }}
              />
            </div>
          </div>
          {form.environment === "production" && !form.readOnly && (
            <div className="flex items-center gap-2 rounded-lg bg-warn-soft px-2.5 py-1.5 text-xs text-warn">
              <TriangleAlert className="size-3.5 shrink-0" /> {t("connection.prodWarning")}
            </div>
          )}
          <div className="border-t border-border pt-2.5 text-xs leading-snug text-fg-subtle">
            <span className="font-medium text-fg-muted">{t("connection.ssh")}</span> — {t("connection.sshPlanned")}
          </div>
        </fieldset>

        <Field label={t("connection.tags")} optional={t("common.optional")}>
          <Input value={tags} placeholder={t("connection.tagsPlaceholder")} onChange={(e) => setTags(e.target.value)} />
        </Field>

        <div aria-live="polite">
          {test.status === "ok" && (
            <div data-test-result="ok" className="flex items-start gap-2 rounded-lg bg-ok-soft px-3 py-2 text-[13px] text-ok">
              <CircleCheck className="mt-0.5 size-4 shrink-0" />
              <span data-selectable className="min-w-0 flex-1">
                {t("connection.testOk", { engine: t(`engine.${test.info.engine}`), version: test.info.version, ms: test.info.connectMs })}
                {test.info.tls !== null && <> · {test.info.tls ? t("connection.tlsOn") : t("connection.tlsOff")}</>}
              </span>
            </div>
          )}
          {(test.status === "error" || error) && (
            <div role="alert" data-test-result="error" className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
              <span data-selectable className="min-w-0 flex-1 break-words">
                {error ?? (test.status === "error" ? test.message : "")}
              </span>
            </div>
          )}
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
        <Button loading={test.status === "running"} disabled={!complete} onClick={() => void runTest()}>
          {test.status === "running" ? t("connection.testing") : t("connection.test")}
        </Button>
        <div className="flex-1" />
        <Dialog.Close asChild>
          <Button variant="ghost">{t("common.cancel")}</Button>
        </Dialog.Close>
        <Button type="submit" variant="primary" loading={saving} disabled={!complete}>
          {existing ? t("common.save") : t("connection.create")}
        </Button>
      </div>
    </form>
  );
}
