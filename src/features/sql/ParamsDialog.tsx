import { Braces } from "lucide-react";
import { Dialog } from "radix-ui";
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, Input, inputClass } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import type { ParamValue } from "../../lib/types";
import { autoKind, type ParamDraft, type ParamKind, toValues } from "./params";

/**
 * The values of a script's named parameters (`:name`), asked before it runs. Each is written
 * into the script as a literal: a number, a quoted text, or NULL.
 */
export function ParamsDialog({
  names,
  initial,
  onCancel,
  onSubmit,
}: {
  names: string[] | null;
  initial: Record<string, ParamDraft>;
  onCancel: () => void;
  onSubmit: (values: Record<string, ParamValue>, drafts: Record<string, ParamDraft>) => void;
}) {
  return (
    <Dialog.Root open={names !== null} onOpenChange={(open) => !open && onCancel()}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          data-sql-params
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex max-h-[80vh] w-[min(94vw,600px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          {names && <Form names={names} initial={initial} onCancel={onCancel} onSubmit={onSubmit} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Form({
  names,
  initial,
  onCancel,
  onSubmit,
}: {
  names: string[];
  initial: Record<string, ParamDraft>;
  onCancel: () => void;
  onSubmit: (values: Record<string, ParamValue>, drafts: Record<string, ParamDraft>) => void;
}) {
  const { t } = useTranslation();
  const [drafts, setDrafts] = useState<Record<string, ParamDraft>>(() =>
    Object.fromEntries(names.map((n) => [n, initial[n] ?? { kind: "auto" as ParamKind, value: "" }])),
  );
  const [problem, setProblem] = useState<string | null>(null);
  const set = (name: string, change: Partial<ParamDraft>) => setDrafts((d) => ({ ...d, [name]: { ...d[name]!, ...change } }));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const out = toValues(names, drafts);
    if ("notNumber" in out) return setProblem(t("params.notNumber", { name: out.notNumber }));
    onSubmit(out.values, drafts);
  };

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-border px-5 py-3">
        <Braces className="size-5 shrink-0 text-accent-text" />
        <Dialog.Title className="text-[15px] font-semibold">{t("params.title")}</Dialog.Title>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-5 py-4">
        <p className="text-xs leading-relaxed text-fg-muted">{t("params.hint")}</p>
        {names.map((name, i) => {
          const d = drafts[name]!;
          const shown = d.kind === "auto" ? autoKind(d.value) : d.kind;
          return (
            <div key={name} data-param={name} className="flex items-center gap-2">
              <span className="w-32 shrink-0 truncate font-mono text-[13px] font-medium" title={`:${name}`}>
                :{name}
              </span>
              <span className="min-w-0 flex-1">
                <Input
                  autoFocus={i === 0}
                  aria-label={`:${name}`}
                  data-param-value={name}
                  value={d.value}
                  disabled={d.kind === "null"}
                  placeholder={d.kind === "null" ? "NULL" : ""}
                  onChange={(e) => set(name, { value: e.target.value })}
                  className="h-8 font-mono"
                />
              </span>
              <span className="w-44 shrink-0">
                <select
                  aria-label={t("params.kind", { name })}
                  data-param-kind={name}
                  value={d.kind}
                  onChange={(e) => set(name, { kind: e.target.value as ParamKind })}
                  className={cn(inputClass, "h-8 text-xs")}
                >
                  <option value="auto">{t("params.auto", { kind: t(`params.${shown}`) })}</option>
                  <option value="text">{t("params.text")}</option>
                  <option value="number">{t("params.number")}</option>
                  <option value="null">NULL</option>
                </select>
              </span>
            </div>
          );
        })}
        {problem && <p className="text-xs text-danger">{problem}</p>}
      </div>
      <div className="flex shrink-0 justify-end gap-2 border-t border-border px-5 py-3">
        <Button variant="ghost" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button type="submit" variant="primary" data-params-run>
          {t("params.run")}
        </Button>
      </div>
    </form>
  );
}
