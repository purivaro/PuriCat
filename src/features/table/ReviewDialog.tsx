import { CircleAlert, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { EngineTag, EnvBadge } from "../../components/Brand";
import { Button, IconButton, Spinner } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { errorMessage, ipc } from "../../lib/ipc";
import type { ApplyResult, ChangeSet, ConnectionView, Statement, TableRef } from "../../lib/types";
import type { Summary } from "./pending";

/** Statements shown in full; the rest are counted. All of them run. */
const PREVIEW_LIMIT = 60;

const KIND_TONE: Record<Statement["kind"], string> = {
  update: "bg-warn-soft text-warn",
  insert: "bg-ok-soft text-ok",
  delete: "bg-danger-soft text-danger",
};

interface Props {
  open: boolean;
  onClose: () => void;
  connection: ConnectionView;
  table: TableRef;
  changes: ChangeSet;
  summary: Summary;
  onSaved: (result: ApplyResult) => void;
}

/**
 * The review step for bulk, destructive or production saves. It shows the exact
 * parameterised statements the backend will run — the plan comes from the same code path.
 */
export function ReviewDialog({ open, onClose, connection, table, changes, summary, onSaved }: Props) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<Statement[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const production = connection.environment === "production";

  useEffect(() => {
    if (!open) return;
    let stale = false;
    setPlan(null);
    setError(null);
    setSaving(false);
    ipc
      .planChanges(connection.id, table, changes)
      .then((statements) => !stale && setPlan(statements))
      .catch((e: unknown) => !stale && setError(errorMessage(e)));
    return () => {
      stale = true;
    };
    // `changes` is rebuilt on every render; the dialog plans once per opening.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const commit = async () => {
    setSaving(true);
    setError(null);
    try {
      onSaved(await ipc.applyChanges(connection.id, table, changes));
    } catch (e) {
      setError(errorMessage(e));
      setSaving(false);
    }
  };

  const counts = [
    summary.editedCells > 0 && t("table.changedCells", { count: summary.editedCells }),
    summary.inserted > 0 && t("table.changedInserted", { count: summary.inserted }),
    summary.deleted > 0 && t("table.changedDeleted", { count: summary.deleted }),
  ].filter(Boolean);

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && !saving && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-review-dialog
          tabIndex={-1}
          // Focus the dialog itself: focusing the first button would pop its tooltip open.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[min(88vh,720px)] w-[min(94vw,760px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          <div className="flex shrink-0 items-center gap-3 border-b border-border py-3 pr-3 pl-5">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="truncate text-[15px] font-semibold">{t("review.title", { name: table.name })}</Dialog.Title>
              <div className="mt-0.5 truncate text-xs text-fg-muted">{counts.join(" · ")}</div>
            </div>
            <div className="flex h-7 max-w-56 shrink-0 items-center gap-1.5 rounded-lg border border-border px-1.5 text-xs">
              <EngineTag engine={connection.engine} />
              <span className="truncate font-medium">{connection.name}</span>
              <EnvBadge environment={connection.environment} long />
            </div>
            <Dialog.Close asChild>
              <IconButton label={t("common.close")} disabled={saving}>
                <X className="size-4" />
              </IconButton>
            </Dialog.Close>
          </div>

          <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-5 py-4">
            {production && (
              <div className="flex items-center gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] font-medium text-danger">
                <TriangleAlert className="size-4 shrink-0" /> {t("review.prodWarning")}
              </div>
            )}
            <p className="text-xs leading-relaxed text-fg-muted">{t("review.transaction")}</p>

            {!plan && !error && (
              <div className="flex items-center gap-2 py-6 text-[13px] text-fg-muted">
                <Spinner className="size-4" /> {t("review.loading")}
              </div>
            )}

            {plan && (
              <section>
                <h3 className="mb-1.5 text-xs font-semibold tracking-wide text-fg-subtle uppercase">
                  {t("review.statements", { n: plan.length })}
                </h3>
                <ol data-selectable className="flex flex-col gap-2">
                  {plan.slice(0, PREVIEW_LIMIT).map((s, i) => (
                    <li key={i} className="overflow-hidden rounded-lg border border-border">
                      <div className="flex items-center gap-2 border-b border-border bg-sunken px-2.5 py-1 text-xs">
                        <span className={cn("rounded px-1.5 py-px text-[10px] font-bold uppercase", KIND_TONE[s.kind])}>{s.kind}</span>
                        <span className="truncate font-mono text-fg-muted">{s.row}</span>
                      </div>
                      <pre className="px-2.5 py-1.5 font-mono text-[11.5px] leading-relaxed break-words whitespace-pre-wrap text-fg">{s.sql}</pre>
                      {s.params.length > 0 && (
                        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-t border-border px-2.5 py-1.5 font-mono text-[11px]">
                          <span className="font-sans text-fg-subtle">{t("review.params")}</span>
                          {s.params.map((p, j) => (
                            <span key={j} className="max-w-full break-words">
                              <span className="text-fg-subtle">{j + 1}=</span>
                              {p === null ? (
                                <span className="text-fg-subtle italic">NULL</span>
                              ) : (
                                <span className="text-accent-text">{JSON.stringify(p)}</span>
                              )}
                            </span>
                          ))}
                        </div>
                      )}
                    </li>
                  ))}
                </ol>
                {plan.length > PREVIEW_LIMIT && (
                  <p className="mt-2 text-xs text-fg-muted">{t("review.more", { n: plan.length - PREVIEW_LIMIT })}</p>
                )}
              </section>
            )}

            {error && (
              <div role="alert" data-review-error className="flex items-start gap-2 rounded-lg bg-danger-soft px-3 py-2 text-[13px] text-danger">
                <CircleAlert className="mt-0.5 size-4 shrink-0" />
                <span data-selectable className="min-w-0 flex-1 break-words">
                  {error}
                </span>
              </div>
            )}
          </div>

          <div className="flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3">
            <Dialog.Close asChild>
              <Button variant="ghost" disabled={saving}>
                {t("common.cancel")}
              </Button>
            </Dialog.Close>
            <Button variant={production ? "danger" : "primary"} loading={saving} disabled={!plan} onClick={() => void commit()}>
              {production ? t("review.commitProd") : t("review.commit")}
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
