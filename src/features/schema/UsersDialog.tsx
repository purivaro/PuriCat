import {
  ArrowLeft,
  Check,
  CircleCheck,
  Copy,
  Dices,
  Eye,
  EyeOff,
  KeyRound,
  Lock,
  Minus,
  Plus,
  Search,
  ShieldCheck,
  Trash2,
  TriangleAlert,
  UserRound,
  UserRoundPlus,
  UsersRound,
} from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, type ReactNode, type RefObject, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button, Field, IconButton, Input, inputClass, Spinner } from "../../components/ui/primitives";
import { clipboard } from "../../lib/clipboard";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { Account, AccountDetails, AccountRef, ConnectionView, UserSpec, UserStep, UserTemplate } from "../../lib/types";
import { useUi } from "../../stores/ui";
import { Failure, Header, Notice } from "./parts";
import { accountLabel, filterAccounts, generatePassword, groupSteps, sameAccount, untouchable, type UserDraft, userSpec } from "./users";

/**
 * The accounts of a server: who they are, what they may do, and three forms — a new user
 * from a template, another password, dropping one. As in the other forms of the schema
 * menus, a form only chooses: every statement is built by the backend, shown here as it
 * will run, and run by the same code. A password is shown as a mask unless asked for.
 */
export function UsersDialog() {
  const connection = useUi((s) => s.usersOf);
  const close = () => useUi.getState().askUsers(null);
  const [busy, setBusy] = useState(false);
  // Set while a form or a new password is on screen. A click outside then does nothing, and
  // Escape does what the screen's own way back does: a form returns to the list (as its Back
  // button does, leaving what was typed), but the screen that shows a new password, the one
  // time it is shown, stays until "Done" — a slip of a key must not throw that away.
  const back = useRef<(() => void) | null>(null);
  return (
    <Dialog.Root open={connection !== null} onOpenChange={(open) => !open && !busy && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-users-dialog
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            const root = e.currentTarget as HTMLElement;
            (root.querySelector<HTMLElement>("input:not([disabled])") ?? root).focus();
          }}
          onEscapeKeyDown={(e) => {
            if (!back.current) return;
            e.preventDefault();
            if (!busy) back.current();
          }}
          onInteractOutside={(e) => back.current && e.preventDefault()}
          tabIndex={-1}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex h-[min(92vh,620px)] w-[min(96vw,900px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {/* Mounted per opening, so the list is read from the server afresh. */}
          {connection && <Users key={connection.id} connection={connection} busy={busy} onBusy={setBusy} back={back} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

type View =
  | { kind: "browse" }
  | { kind: "new" }
  | { kind: "password"; account: Account }
  | { kind: "drop"; account: Account }
  /** A password was set: the one chance to copy it. */
  | { kind: "done"; title: string; password: string };

interface UsersProps {
  connection: ConnectionView;
  busy: boolean;
  onBusy: (busy: boolean) => void;
  back: RefObject<(() => void) | null>;
}

function Users({ connection, busy, onBusy, back }: UsersProps) {
  const { t } = useTranslation();
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selected, setSelected] = useState<AccountRef | null>(null);
  const [view, setView] = useState<View>({ kind: "browse" });
  const browse = useCallback(() => setView({ kind: "browse" }), []);

  const reload = useCallback(async () => {
    try {
      setAccounts(await ipc.listAccounts(connection.id));
      setLoadError(null);
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, [connection.id]);
  useEffect(() => void reload(), [reload]);

  useEffect(() => {
    back.current = view.kind === "browse" ? null : view.kind === "done" ? () => undefined : browse;
    return () => {
      back.current = null;
    };
  }, [back, view.kind, browse]);

  const title =
    view.kind === "new"
      ? t("users.newTitle")
      : view.kind === "password"
        ? t("users.passwordTitle", { name: accountLabel(view.account) })
        : view.kind === "drop"
          ? t("users.dropTitle", { name: accountLabel(view.account) })
          : t("users.title");
  const icon =
    view.kind === "drop" ? (
      <Trash2 className="size-5 shrink-0 text-danger" />
    ) : view.kind === "new" ? (
      <UserRoundPlus className="size-5 shrink-0 text-accent-text" />
    ) : view.kind === "password" ? (
      <KeyRound className="size-5 shrink-0 text-accent-text" />
    ) : (
      <UsersRound className="size-5 shrink-0 text-accent-text" />
    );
  const you = accounts?.find((a) => a.you)?.name ?? null;
  const form = { connection, onBusy, onBack: browse };

  return (
    <div data-users-view={view.kind} className="flex min-h-0 flex-1 flex-col">
      <Header icon={icon} title={title} connection={connection} busy={busy} />
      {view.kind === "browse" && (
        <Browse
          connection={connection}
          accounts={accounts}
          loadError={loadError}
          selected={selected}
          onSelect={setSelected}
          onNew={() => setView({ kind: "new" })}
          onPassword={(account) => setView({ kind: "password", account })}
          onDrop={(account) => setView({ kind: "drop", account })}
        />
      )}
      {view.kind === "new" && (
        <NewUserForm
          {...form}
          you={you}
          onCreated={async (account, password) => {
            await reload();
            setSelected(account);
            setView({ kind: "done", title: t("users.createdTitle", { name: accountLabel(account) }), password });
          }}
        />
      )}
      {view.kind === "password" && (
        <PasswordForm
          {...form}
          account={view.account}
          onChanged={(password) => setView({ kind: "done", title: t("users.changedTitle", { name: accountLabel(view.account) }), password })}
        />
      )}
      {view.kind === "drop" && (
        <DropForm
          {...form}
          account={view.account}
          onDropped={async () => {
            setSelected(null);
            await reload();
            browse();
          }}
        />
      )}
      {view.kind === "done" && <Done title={view.title} password={view.password} onDone={browse} />}
    </div>
  );
}

// ---- The list and what one account may do ------------------------------------------

function Badge({ tone = "muted", children }: { tone?: "muted" | "accent" | "warn"; children: ReactNode }) {
  return (
    <span
      className={cn(
        "shrink-0 rounded px-1 py-px text-[10px] leading-4 font-semibold whitespace-nowrap",
        tone === "muted" && "bg-sunken text-fg-muted",
        tone === "accent" && "bg-accent-soft text-accent-text",
        tone === "warn" && "bg-warn-soft text-warn",
      )}
    >
      {children}
    </span>
  );
}

function Badges({ account, engine }: { account: Account; engine: ConnectionView["engine"] }) {
  const { t } = useTranslation();
  return (
    <>
      {account.you && <Badge tone="accent">{t("users.you")}</Badge>}
      {account.superuser && <Badge tone="warn">{t("users.superuser")}</Badge>}
      {account.system && <Badge>{t("users.system")}</Badge>}
      {account.locked && !account.system && <Badge>{t(engine === "mysql" ? "users.locked" : "users.noLogin")}</Badge>}
    </>
  );
}

interface BrowseProps {
  connection: ConnectionView;
  accounts: Account[] | null;
  loadError: string | null;
  selected: AccountRef | null;
  onSelect: (account: AccountRef) => void;
  onNew: () => void;
  onPassword: (account: Account) => void;
  onDrop: (account: Account) => void;
}

function Browse({ connection, accounts, loadError, selected, onSelect, onNew, onPassword, onDrop }: BrowseProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const shown = useMemo(() => filterAccounts(accounts ?? [], query), [accounts, query]);
  const current = accounts?.find((a) => selected && sameAccount(a, selected)) ?? null;

  return (
    <div className="flex min-h-0 flex-1">
      <div className="flex w-[300px] shrink-0 flex-col border-r border-border">
        <div className="relative p-3 pb-2">
          <Search className="pointer-events-none absolute top-1/2 left-5.5 size-3.5 -translate-y-[calc(50%-2px)] text-fg-subtle" />
          <Input aria-label={t("users.search")} placeholder={t("users.search")} value={query} onChange={(e) => setQuery(e.target.value)} className="pl-8" />
        </div>
        <div role="listbox" aria-label={t("users.title")} className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2 pb-2">
          {loadError ? (
            <div className="p-1">
              <Failure message={t("users.loadFailed", { message: loadError })} />
            </div>
          ) : !accounts ? (
            <div className="flex items-center gap-2 px-2 py-4 text-[13px] text-fg-muted">
              <Spinner className="size-4" /> {t("common.loading")}
            </div>
          ) : shown.length === 0 ? (
            <p className="px-2 py-4 text-[13px] text-fg-subtle">{t("users.noMatch")}</p>
          ) : (
            shown.map((account) => {
              const active = !!selected && sameAccount(account, selected);
              const Icon = account.system || account.locked ? Lock : account.superuser ? ShieldCheck : UserRound;
              return (
                <button
                  key={accountLabel(account)}
                  type="button"
                  role="option"
                  aria-selected={active}
                  data-account-row={accountLabel(account)}
                  title={accountLabel(account)}
                  onClick={() => onSelect(account)}
                  className={cn(
                    "flex h-8 shrink-0 items-center gap-2 rounded-md px-2 text-left text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-(--ring)",
                    active ? "bg-accent-soft text-accent-text" : "hover:bg-sunken",
                    (account.system || account.locked) && !active && "text-fg-muted",
                  )}
                >
                  <Icon className={cn("size-3.5 shrink-0", active ? "text-accent-text" : "text-fg-subtle")} />
                  <span className="min-w-0 flex-1 truncate">
                    <span className="font-medium">{account.name}</span>
                    {account.host && <span className={active ? "opacity-70" : "text-fg-subtle"}>@{account.host}</span>}
                  </span>
                  <Badges account={account} engine={connection.engine} />
                </button>
              );
            })
          )}
        </div>
        <div className="border-t border-border p-3">
          <Button
            variant="primary"
            icon={<Plus className="size-4" />}
            className="w-full"
            disabled={connection.readOnly}
            title={connection.readOnly ? t("users.readOnly") : undefined}
            onClick={onNew}
          >
            {t("users.new")}
          </Button>
        </div>
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        {current ? (
          <AccountPane key={accountLabel(current)} connection={connection} account={current} onPassword={onPassword} onDrop={onDrop} />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
            <div className="flex size-11 items-center justify-center rounded-2xl bg-accent-soft text-accent-text">
              <UsersRound className="size-5" />
            </div>
            <p className="max-w-72 text-[13px] text-fg-muted">{t("users.empty")}</p>
          </div>
        )}
      </div>
    </div>
  );
}

function Yes({ on }: { on: boolean }) {
  return on ? <Check aria-label="yes" className="size-3.5 text-ok" /> : <Minus aria-label="no" className="size-3.5 text-border-strong" />;
}

function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h3 className="text-xs font-medium text-fg-muted">{title}</h3>
      {children}
      {note && <p className="text-xs leading-snug text-fg-subtle">{note}</p>}
    </section>
  );
}

interface PaneProps {
  connection: ConnectionView;
  account: Account;
  onPassword: (account: Account) => void;
  onDrop: (account: Account) => void;
}

function AccountPane({ connection, account, onPassword, onDrop }: PaneProps) {
  const { t } = useTranslation();
  const [details, setDetails] = useState<AccountDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mysql = connection.engine === "mysql";

  useEffect(() => {
    let stale = false;
    ipc
      .accountDetails(connection.id, { name: account.name, host: account.host })
      .then((d) => !stale && setDetails(d))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
  }, [connection.id, account.name, account.host]);

  // Why the two actions are not offered for this account, if they are not.
  const why = connection.readOnly
    ? t("users.readOnly")
    : account.you
      ? t("users.untouchable.you")
      : account.system
        ? t("users.untouchable.system")
        : account.superuser
          ? t("users.untouchable.superuser")
          : undefined;
  const fixed = connection.readOnly || untouchable(account);
  const attributes = details?.attributes;

  return (
    <>
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-5 py-3">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1">
          <span data-selectable data-account-name className="truncate font-mono text-[13px] font-semibold">
            {accountLabel(account)}
          </span>
          <Badges account={account} engine={connection.engine} />
        </div>
        <Button size="sm" icon={<KeyRound className="size-3.5" />} disabled={fixed} title={why} onClick={() => onPassword(account)}>
          {t("users.changePassword")}
        </Button>
        <Button
          size="sm"
          icon={<Trash2 className="size-3.5" />}
          disabled={fixed}
          title={why}
          className={cn(!fixed && "text-danger hover:bg-danger-soft")}
          onClick={() => onDrop(account)}
        >
          {t("users.drop")}
        </Button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        {error ? (
          <Failure message={error} />
        ) : !details ? (
          <div className="flex items-center gap-2 py-6 text-[13px] text-fg-muted">
            <Spinner className="size-4" /> {t("common.loading")}
          </div>
        ) : mysql ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13px]">
              <dt className="text-fg-muted">{t("users.plugin")}</dt>
              <dd className="font-mono text-xs leading-5">{details.plugin ?? "—"}</dd>
              <dt className="text-fg-muted">{t("users.accountLocked")}</dt>
              <dd>{t(account.locked ? "users.yes" : "users.no")}</dd>
              <dt className="text-fg-muted">{t("users.passwordExpired")}</dt>
              <dd>{t(details.passwordExpired ? "users.yes" : "users.no")}</dd>
            </dl>
            <Section title={t("users.grants")} note={t("users.grantsNote")}>
              <pre
                data-selectable
                data-grants
                className="rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap"
              >
                {details.grants.map((g) => `${g};`).join("\n")}
              </pre>
            </Section>
          </>
        ) : (
          attributes && (
            <>
              <Section title={t("users.attributes")}>
                <ul className="grid grid-cols-2 gap-x-4 gap-y-1 text-[13px]">
                  {(
                    [
                      ["login", attributes.login],
                      ["superuser", attributes.superuser],
                      ["createDatabase", attributes.createDatabase],
                      ["createRole", attributes.createRole],
                      ["replication", attributes.replication],
                      ["bypassRls", attributes.bypassRls],
                    ] as const
                  ).map(([key, on]) => (
                    <li key={key} className={cn("flex items-center gap-2", !on && "text-fg-subtle")}>
                      <Yes on={on} /> {t(`users.attribute.${key}`)}
                    </li>
                  ))}
                </ul>
                <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13px]">
                  <dt className="text-fg-muted">{t("users.connectionLimit")}</dt>
                  <dd>{attributes.connectionLimit < 0 ? t("users.noLimit") : attributes.connectionLimit}</dd>
                  <dt className="text-fg-muted">{t("users.validUntil")}</dt>
                  <dd>{attributes.validUntil ?? t("users.forever")}</dd>
                  <dt className="text-fg-muted">{t("users.memberOf")}</dt>
                  <dd data-selectable>{details.memberOf.join(", ") || "—"}</dd>
                  <dt className="text-fg-muted">{t("users.members")}</dt>
                  <dd data-selectable>{details.members.join(", ") || "—"}</dd>
                </dl>
              </Section>
              <Section title={t("users.databases")} note={t("users.tablesNote")}>
                <div className="overflow-hidden rounded-lg border border-border">
                  <table data-database-access className="w-full text-[13px]">
                    <thead className="bg-sunken text-xs text-fg-muted">
                      <tr>
                        <th className="px-3 py-1.5 text-left font-medium">{t("users.database")}</th>
                        {(["owner", "connect", "create", "temporary"] as const).map((key) => (
                          <th key={key} className="w-20 px-2 py-1.5 text-center font-medium">
                            {t(`users.access.${key}`)}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {details.databases.map((d) => (
                        <tr key={d.name} className="border-t border-border">
                          <td data-selectable className="max-w-0 truncate px-3 py-1.5 font-mono text-xs">
                            {d.name}
                          </td>
                          {[d.owner, d.connect, d.create, d.temporary].map((on, i) => (
                            <td key={i} className="px-2 py-1.5">
                              <span className="flex justify-center">
                                <Yes on={on} />
                              </span>
                            </td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </Section>
            </>
          )
        )}
      </div>
    </>
  );
}

// ---- Pieces of the forms -----------------------------------------------------------

interface PasswordFieldProps {
  value: string;
  onChange: (value: string) => void;
  shown: boolean;
  onShown: (shown: boolean) => void;
  disabled?: boolean;
  /** A password that is only shown to be copied cannot be typed over or made anew. */
  fixed?: boolean;
}

function PasswordField({ value, onChange, shown, onShown, disabled, fixed }: PasswordFieldProps) {
  const { t } = useTranslation();
  const copy = () =>
    void clipboard
      .write(value)
      .then(() => toast.success(t("common.copied")))
      .catch((e: unknown) => toast.error(errorMessage(e)));
  return (
    <div className="flex items-center gap-1.5">
      <Input
        data-password
        type={shown ? "text" : "password"}
        value={value}
        readOnly={fixed}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        className="min-w-0 flex-1 font-mono"
      />
      <IconButton label={t(shown ? "users.hide" : "users.show")} disabled={disabled} onClick={() => onShown(!shown)} className="size-8 border border-border-strong">
        {shown ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
      </IconButton>
      <IconButton label={t("users.copyPassword")} disabled={disabled || !value} onClick={copy} className="size-8 border border-border-strong">
        <Copy className="size-4" />
      </IconButton>
      {!fixed && (
        <Button
          icon={<Dices className="size-4" />}
          disabled={disabled}
          onClick={() => {
            onChange(generatePassword());
            // A password nobody chose has to be seen to be kept somewhere.
            onShown(true);
          }}
        >
          {t("users.generate")}
        </Button>
      )}
    </div>
  );
}

type Planned = { steps: UserStep[] } | { problem: string } | null;

/** The backend's statements for the choices on screen, in the groups they run in. */
function Steps({ plan, empty, stale, revealed }: { plan: Planned; empty: string; stale: boolean; revealed: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-medium text-fg-muted">{t("users.statements")}</span>
      {plan && "steps" in plan ? (
        <div data-ddl-preview aria-busy={stale} className={cn("flex flex-col gap-1.5 transition-opacity", stale && "opacity-50")}>
          {groupSteps(plan.steps).map((group, i) => (
            <div key={i} className="flex flex-col gap-1">
              {group.database && <span className="text-[11px] text-fg-subtle">{t("users.inside", { name: group.database })}</span>}
              <pre data-selectable className="rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap">
                {group.sql.map((sql) => `${sql};`).join("\n")}
              </pre>
            </div>
          ))}
          <p className="text-xs leading-snug text-fg-subtle">{t(revealed ? "users.revealNote" : "users.maskNote")}</p>
        </div>
      ) : (
        <div data-ddl-preview-empty className="rounded-lg border border-dashed border-border px-3 py-2 text-xs text-fg-subtle">
          {plan ? <span className="text-danger">{plan.problem}</span> : empty}
        </div>
      )}
    </div>
  );
}

function FormFooter({ busy, onBack, children }: { busy: boolean; onBack: () => void; children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 items-center gap-2 border-t border-border px-5 py-3">
      <Button variant="ghost" icon={<ArrowLeft className="size-4" />} disabled={busy} onClick={onBack}>
        {t("users.back")}
      </Button>
      <span className="flex-1" />
      {children}
    </div>
  );
}

function Guards({ connection }: { connection: ConnectionView }) {
  const { t } = useTranslation();
  if (connection.readOnly) {
    return (
      <Notice tone="muted" icon={<Lock className="size-4" />}>
        {t("users.readOnly")}
      </Notice>
    );
  }
  if (connection.environment !== "production") return null;
  return (
    <Notice tone="danger" icon={<TriangleAlert className="size-4" />}>
      {t("users.prodWarning")}
    </Notice>
  );
}

interface FormProps {
  connection: ConnectionView;
  onBusy: (busy: boolean) => void;
  onBack: () => void;
}

// ---- New user ----------------------------------------------------------------------

const TEMPLATES: UserTemplate[] = ["app", "readOnly", "none"];

function NewUserForm({ connection, onBusy, onBack, you, onCreated }: FormProps & { you: string | null; onCreated: (account: AccountRef, password: string) => Promise<void> }) {
  const { t } = useTranslation();
  const mysql = connection.engine === "mysql";
  const production = connection.environment === "production";

  const [template, setTemplate] = useState<UserTemplate>("app");
  const [name, setName] = useState("");
  const [hostChoice, setHostChoice] = useState<"%" | "localhost" | "custom">("%");
  const [customHost, setCustomHost] = useState("");
  const [database, setDatabase] = useState("");
  const [password, setPassword] = useState("");
  const [shown, setShown] = useState(false);
  const [databases, setDatabases] = useState<string[] | null>(null);
  const [plan, setPlan] = useState<(NonNullable<Planned> & { spec: UserSpec; shown: boolean }) | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    ipc
      .listDatabases(connection.id)
      .then((list) => !stale && setDatabases(list.filter((d) => !d.system).map((d) => d.name)))
      .catch(() => !stale && setDatabases([]));
    return () => {
      stale = true;
    };
  }, [connection.id]);

  const draft = useMemo<UserDraft>(
    () => ({ name, host: hostChoice === "custom" ? customHost : hostChoice, template, database, password }),
    [name, hostChoice, customHost, template, database, password],
  );
  const spec = useMemo(() => userSpec(connection.engine, draft), [connection.engine, draft]);
  // Enough is chosen for the backend to write the statements.
  const complete = name.trim() !== "" && (template === "none" || database !== "");

  useEffect(() => {
    setError(null);
    if (!complete) return setPlan(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planUser(connection.id, spec, shown)
        .then((steps) => !stale && setPlan({ spec, shown, steps }))
        .catch((e: unknown) => !stale && setPlan({ spec, shown, problem: errorMessage(e) }));
    }, 150);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, spec, shown, complete]);

  const current = plan?.spec === spec && plan.shown === shown;
  const ready = !!plan && current && "steps" in plan && password !== "" && !connection.readOnly && !creating;
  const escaped = mysql && template !== "none" && /[_%]/.test(database) && !!plan && "steps" in plan && plan.steps.some((s) => s.sql.includes("\\"));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setCreating(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.createUser(connection.id, spec);
      const account = { name: name.trim(), host: mysql ? draft.host.trim() || "%" : null };
      toast.success(t("users.created", { name: accountLabel(account) }));
      await onCreated(account, password);
    } catch (err) {
      setError(errorMessage(err));
      setCreating(false);
    } finally {
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-1 flex-col">
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,5fr)_minmax(0,6fr)] divide-x divide-border">
        <div className="flex min-h-0 flex-col gap-3.5 overflow-y-auto px-5 py-4">
          <div role="radiogroup" aria-label={t("users.template")} className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-fg-muted">{t("users.template")}</span>
            {TEMPLATES.map((kind) => {
              const on = template === kind;
              return (
                <button
                  key={kind}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  data-template={kind}
                  disabled={creating}
                  onClick={() => setTemplate(kind)}
                  className={cn(
                    "flex items-start gap-2.5 rounded-lg border px-3 py-2 text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-(--ring)",
                    on ? "border-accent bg-accent-soft" : "border-border hover:bg-sunken",
                  )}
                >
                  <span className={cn("mt-0.5 flex size-3.5 shrink-0 items-center justify-center rounded-full border", on ? "border-accent" : "border-border-strong")}>
                    {on && <span className="size-2 rounded-full bg-accent" />}
                  </span>
                  <span className="min-w-0">
                    <span className="block text-[13px] font-medium">{t(`users.templates.${kind}.title`)}</span>
                    <span className="block text-xs leading-snug text-fg-muted">{t(`users.templates.${kind}.hint`)}</span>
                  </span>
                </button>
              );
            })}
          </div>

          <Field label={t("users.name")}>
            <Input autoFocus value={name} disabled={creating} placeholder={t("users.namePlaceholder")} onChange={(e) => setName(e.target.value)} />
          </Field>
          {mysql && (
            <Field label={t("users.host")} hint={t("users.hostHint")}>
              <div className="flex gap-1.5">
                <select
                  value={hostChoice}
                  disabled={creating}
                  onChange={(e) => setHostChoice(e.target.value as typeof hostChoice)}
                  className={cn(inputClass, hostChoice === "custom" && "w-44 shrink-0")}
                >
                  <option value="%">{t("users.hosts.any")}</option>
                  <option value="localhost">{t("users.hosts.local")}</option>
                  <option value="custom">{t("users.hosts.custom")}</option>
                </select>
                {hostChoice === "custom" && (
                  <Input
                    aria-label={t("users.hosts.custom")}
                    value={customHost}
                    disabled={creating}
                    placeholder={t("users.hostPlaceholder")}
                    onChange={(e) => setCustomHost(e.target.value)}
                  />
                )}
              </div>
            </Field>
          )}
          {template !== "none" && (
            <Field label={t("users.database")}>
              <select value={database} disabled={creating || !databases} onChange={(e) => setDatabase(e.target.value)} className={inputClass}>
                <option value="">{t("users.chooseDatabase")}</option>
                {databases?.map((d) => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </select>
            </Field>
          )}
          <Field label={t("users.password")}>
            <PasswordField value={password} onChange={setPassword} shown={shown} onShown={setShown} disabled={creating} />
          </Field>
        </div>

        <div className="flex min-h-0 flex-col gap-3 overflow-y-auto px-5 py-4">
          <Guards connection={connection} />
          <Steps plan={plan} empty={t("users.statementsEmpty")} stale={!current} revealed={shown} />
          {escaped && <p className="text-xs leading-snug text-fg-subtle">{t("users.wildcardNote")}</p>}
          {!mysql && template !== "none" && (
            <ul className="flex list-disc flex-col gap-1 pl-4 text-xs leading-snug text-fg-subtle">
              <li>{t("users.pgDefaultNote", { role: you ?? "…" })}</li>
              <li>{t("users.pgPublicNote")}</li>
            </ul>
          )}
          <Failure message={error} />
        </div>
      </div>
      <FormFooter busy={creating} onBack={onBack}>
        <Button type="submit" variant={production ? "danger" : "primary"} loading={creating} disabled={!ready}>
          {creating ? t("users.creating") : t("users.create")}
        </Button>
      </FormFooter>
    </form>
  );
}

// ---- Another password --------------------------------------------------------------

function PasswordForm({ connection, onBusy, onBack, account, onChanged }: FormProps & { account: Account; onChanged: (password: string) => void }) {
  const { t } = useTranslation();
  const production = connection.environment === "production";
  const who = useMemo<AccountRef>(() => ({ name: account.name, host: account.host }), [account.name, account.host]);
  const [password, setPassword] = useState("");
  const [shown, setShown] = useState(false);
  const [plan, setPlan] = useState<(NonNullable<Planned> & { password: string; shown: boolean }) | null>(null);
  const [changing, setChanging] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    let stale = false;
    const timer = window.setTimeout(() => {
      ipc
        .planPassword(connection.id, who, password, shown)
        .then((sql) => !stale && setPlan({ password, shown, steps: [{ sql, database: null }] }))
        .catch((e: unknown) => !stale && setPlan({ password, shown, problem: errorMessage(e) }));
    }, 150);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [connection.id, who, password, shown]);

  const current = plan?.password === password && plan.shown === shown;
  const ready = !!plan && current && "steps" in plan && password !== "" && !connection.readOnly && !changing;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setChanging(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.changePassword(connection.id, who, password);
      toast.success(t("users.changed", { name: accountLabel(who) }));
      onChanged(password);
    } catch (err) {
      setError(errorMessage(err));
      setChanging(false);
    } finally {
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-1 flex-col">
      <div className="mx-auto flex min-h-0 w-full max-w-[600px] flex-1 flex-col gap-4 overflow-y-auto px-5 py-5">
        <Guards connection={connection} />
        <p className="text-[13px] leading-relaxed">{t("users.passwordAbout")}</p>
        <Field label={t("users.password")}>
          <PasswordField value={password} onChange={setPassword} shown={shown} onShown={setShown} disabled={changing} />
        </Field>
        <Steps plan={plan} empty="…" stale={!current} revealed={shown} />
        <Failure message={error} />
      </div>
      <FormFooter busy={changing} onBack={onBack}>
        <Button type="submit" variant={production ? "danger" : "primary"} loading={changing} disabled={!ready}>
          {changing ? t("users.changing") : t("users.change")}
        </Button>
      </FormFooter>
    </form>
  );
}

// ---- Drop --------------------------------------------------------------------------

function DropForm({ connection, onBusy, onBack, account, onDropped }: FormProps & { account: Account; onDropped: () => Promise<void> }) {
  const { t } = useTranslation();
  const production = connection.environment === "production";
  const who = useMemo<AccountRef>(() => ({ name: account.name, host: account.host }), [account.name, account.host]);
  const label = accountLabel(who);
  const [plan, setPlan] = useState<{ sql: string } | { problem: string } | null>(null);
  const [typed, setTyped] = useState("");
  const [dropping, setDropping] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stale = false;
    ipc
      .planDropUser(connection.id, who)
      .then((sql) => !stale && setPlan({ sql }))
      .catch((e: unknown) => !stale && setPlan({ problem: errorMessage(e) }));
    return () => {
      stale = true;
    };
  }, [connection.id, who]);

  const ready = !!plan && "sql" in plan && typed === label && !connection.readOnly && !dropping;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setDropping(true);
    onBusy(true);
    setError(null);
    try {
      await ipc.dropUser(connection.id, who, typed);
      toast.success(t("users.dropped", { name: label }));
      await onDropped();
    } catch (err) {
      setError(errorMessage(err));
      setDropping(false);
    } finally {
      onBusy(false);
    }
  };

  return (
    <form onSubmit={(e) => void submit(e)} className="flex min-h-0 flex-1 flex-col">
      <div className="mx-auto flex min-h-0 w-full max-w-[600px] flex-1 flex-col gap-4 overflow-y-auto px-5 py-5">
        <Guards connection={connection} />
        <p className="text-[13px] leading-relaxed">{t(`users.dropAbout.${connection.engine === "mysql" ? "mysql" : "postgres"}`)}</p>
        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium text-fg-muted">{t("schema.statement")}</span>
          {plan && "sql" in plan ? (
            <pre data-selectable data-ddl-preview className="rounded-lg border border-border bg-sunken px-3 py-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap">
              {plan.sql};
            </pre>
          ) : (
            <div data-ddl-preview-empty className="rounded-lg border border-dashed border-border px-3 py-2 text-xs text-fg-subtle">
              {plan ? <span className="text-danger">{plan.problem}</span> : "…"}
            </div>
          )}
        </div>
        <Field label={t("schema.dropType", { name: label })}>
          <Input
            autoFocus
            data-confirm-name
            value={typed}
            disabled={dropping || connection.readOnly || !plan || "problem" in plan}
            placeholder={label}
            onChange={(e) => setTyped(e.target.value)}
          />
        </Field>
        <Failure message={error} />
      </div>
      <FormFooter busy={dropping} onBack={onBack}>
        <Button type="submit" variant="danger" loading={dropping} disabled={!ready}>
          {dropping ? t("users.dropping") : production ? t("users.dropProd") : t("users.dropButton")}
        </Button>
      </FormFooter>
    </form>
  );
}

// ---- After a password was set ------------------------------------------------------

function Done({ title, password, onDone }: { title: string; password: string; onDone: () => void }) {
  const { t } = useTranslation();
  const [shown, setShown] = useState(true);
  return (
    <div data-users-done className="flex min-h-0 flex-1 flex-col">
      <div className="mx-auto flex w-full max-w-[560px] flex-1 flex-col justify-center gap-4 px-5 py-6">
        <div className="flex items-center gap-3">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-2xl bg-ok-soft text-ok">
            <CircleCheck className="size-5" />
          </div>
          <h3 className="min-w-0 text-[15px] leading-snug font-semibold break-words">{title}</h3>
        </div>
        <Field label={t("users.password")} hint={t("users.keepPassword")}>
          <PasswordField value={password} onChange={() => undefined} shown={shown} onShown={setShown} fixed />
        </Field>
      </div>
      <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
        <Button variant="primary" autoFocus onClick={onDone}>
          {t("users.done")}
        </Button>
      </div>
    </div>
  );
}
