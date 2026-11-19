import { getVersion } from "@tauri-apps/api/app";
import { CircleAlert, CircleCheck, Download, RotateCw, TriangleAlert, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button, IconButton, Spinner, Switch } from "../../components/ui/primitives";
import { cn } from "../../lib/cn";
import { useSettings } from "../../stores/settings";
import { useUpdates } from "../../stores/updates";
import { useWorkspace } from "../../stores/workspace";

const megabytes = (bytes: number) => (bytes / 1_048_576).toFixed(1);

function Badge({ tone, children }: { tone: "accent" | "ok" | "danger"; children: ReactNode }) {
  return (
    <div
      className={cn(
        "flex size-10 shrink-0 items-center justify-center rounded-2xl",
        tone === "ok" ? "bg-ok-soft text-ok" : tone === "danger" ? "bg-danger-soft text-danger" : "bg-accent-soft text-accent-text",
      )}
    >
      {children}
    </div>
  );
}

/**
 * Everything about updating PuriCat in one place: what the last check found, the download,
 * the restart, and the switch for the check at start-up.
 */
export function UpdateDialog() {
  const { t, i18n } = useTranslation();
  const { status, info, progress, error, failedStep, open, setOpen, check, install, restart } = useUpdates();
  const autoUpdate = useSettings((s) => s.autoUpdate);
  const update = useSettings((s) => s.update);
  // A restart closes every tab: say so first when one of them holds something.
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);
  const unsaved = useWorkspace((s) => new Set([...Object.keys(s.dirty), ...Object.keys(s.edited)]).size);

  const released = (() => {
    const at = info?.date ? new Date(info.date) : null;
    return at && !Number.isNaN(at.getTime()) ? new Intl.DateTimeFormat(i18n.language, { dateStyle: "long" }).format(at) : null;
  })();
  const percent = progress?.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;
  const downloaded = !!progress?.total && progress.downloaded >= progress.total;

  let badge: ReactNode;
  let title: string;
  let body: ReactNode = null;
  let actions: ReactNode = null;

  if (status === "available" && info) {
    badge = (
      <Badge tone="accent">
        <Download className="size-5" />
      </Badge>
    );
    title = t("updates.availableTitle", { version: info.version });
    body = (
      <>
        <p>
          {t("updates.availableBody", { current: info.currentVersion })}
          {released && ` ${t("updates.released", { date: released })}`}
        </p>
        {info.notes && (
          <pre data-selectable className="mt-2 max-h-40 overflow-auto rounded-lg bg-sunken px-3 py-2 font-sans text-xs leading-relaxed whitespace-pre-wrap text-fg">
            {info.notes}
          </pre>
        )}
        <p className="mt-2">{t("updates.about")}</p>
      </>
    );
    actions = (
      <>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          {t("updates.later")}
        </Button>
        <Button variant="primary" data-update-install icon={<Download className="size-3.5" />} onClick={() => void install()}>
          {t("updates.install")}
        </Button>
      </>
    );
  } else if (status === "installing" && info) {
    badge = (
      <Badge tone="accent">
        <Spinner className="size-5" />
      </Badge>
    );
    title = downloaded ? t("updates.verifying") : t("updates.installing", { version: info.version });
    body = (
      <>
        <div
          role="progressbar"
          aria-label={title}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent ?? undefined}
          className="mt-1 h-1.5 overflow-hidden rounded-full bg-sunken"
        >
          <div
            className={cn("h-full rounded-full bg-accent transition-[width] duration-200", percent === null && "w-1/3 animate-pulse")}
            style={percent === null ? undefined : { width: `${percent}%` }}
          />
        </div>
        <p className="mt-2 text-xs tabular-nums">
          {progress
            ? progress.total
              ? t("updates.progress", { done: megabytes(progress.downloaded), total: megabytes(progress.total) })
              : t("updates.progressUnknown", { done: megabytes(progress.downloaded) })
            : " "}
        </p>
      </>
    );
  } else if (status === "ready" && info) {
    badge = (
      <Badge tone="ok">
        <CircleCheck className="size-5" />
      </Badge>
    );
    title = t("updates.readyTitle", { version: info.version });
    body = (
      <>
        <p>{t("updates.readyBody")}</p>
        {unsaved > 0 && (
          <p data-update-unsaved className="mt-2 flex gap-2 rounded-lg bg-warn-soft px-3 py-2 text-warn">
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
            <span>{t("updates.unsaved", { count: unsaved })}</span>
          </p>
        )}
      </>
    );
    actions = (
      <>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          {t("updates.later")}
        </Button>
        <Button variant={unsaved > 0 ? "danger" : "primary"} data-update-restart icon={<RotateCw className="size-3.5" />} onClick={restart}>
          {t(unsaved > 0 ? "updates.restartDiscard" : "updates.restart")}
        </Button>
      </>
    );
  } else if (status === "failed") {
    badge = (
      <Badge tone="danger">
        <CircleAlert className="size-5" />
      </Badge>
    );
    title = t(failedStep === "install" ? "updates.failedInstall" : "updates.failedCheck");
    body = (
      <p data-selectable data-update-error className="break-words">
        {error}
      </p>
    );
    actions = (
      <>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          {t("common.close")}
        </Button>
        <Button variant="secondary" onClick={() => void (failedStep === "install" ? install() : check())}>
          {t("common.retry")}
        </Button>
      </>
    );
  } else if (status === "current") {
    badge = (
      <Badge tone="ok">
        <CircleCheck className="size-5" />
      </Badge>
    );
    title = t("updates.currentTitle");
    body = version && <p>{t("updates.currentBody", { version })}</p>;
    actions = (
      <Button variant="secondary" onClick={() => setOpen(false)}>
        {t("common.close")}
      </Button>
    );
  } else {
    badge = (
      <Badge tone="accent">
        <Spinner className="size-5" />
      </Badge>
    );
    title = t("updates.checking");
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Portal>
        <Dialog.Overlay className="animate-fade fixed inset-0 z-50 bg-black/30 backdrop-blur-[2px] dark:bg-black/55" />
        <Dialog.Content
          data-update-dialog={status}
          aria-describedby={undefined}
          // Focus on the dialog itself: the first control is the close button, and focusing
          // it would open its tooltip over the title.
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            (e.currentTarget as HTMLElement).focus();
          }}
          className="animate-pop fixed top-1/2 left-1/2 z-50 flex w-[min(92vw,460px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl border border-border bg-raised shadow-pop outline-none"
        >
          <div className="flex items-start gap-3.5 py-5 pr-3 pl-5">
            {badge}
            <div className="min-w-0 flex-1 pt-0.5">
              <Dialog.Title className="text-[15px] leading-snug font-semibold">{title}</Dialog.Title>
              {body && <div className="mt-1.5 text-[13px] leading-relaxed text-fg-muted">{body}</div>}
            </div>
            <Dialog.Close asChild>
              <IconButton label={t("common.close")}>
                <X className="size-4" />
              </IconButton>
            </Dialog.Close>
          </div>

          {actions && <div className="flex justify-end gap-2 px-5 pb-4">{actions}</div>}

          <div className="flex items-start gap-3 rounded-b-2xl border-t border-border bg-sunken/50 px-5 py-3.5">
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium">{t("updates.auto")}</div>
              <div className="mt-0.5 text-xs leading-relaxed text-fg-muted">{t("updates.autoHint")}</div>
            </div>
            <Switch checked={autoUpdate} onChange={(v) => update({ autoUpdate: v })} label={t("updates.auto")} />
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
