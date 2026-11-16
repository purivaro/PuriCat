import { LoaderCircle } from "lucide-react";
import { Switch as RSwitch, Tooltip as RTooltip } from "radix-ui";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, Ref } from "react";

import { cn } from "../../lib/cn";

// ---- Button ------------------------------------------------------------------------

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

const BUTTON_VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover shadow-[0_1px_0_rgba(0,0,0,0.06)] font-semibold",
  secondary: "bg-surface text-fg border border-border-strong hover:bg-sunken",
  ghost: "text-fg-muted hover:bg-sunken hover:text-fg",
  danger: "bg-danger text-white hover:brightness-95 font-semibold",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "sm" | "md";
  loading?: boolean;
  icon?: ReactNode;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  variant = "secondary",
  size = "md",
  loading,
  icon,
  className,
  children,
  disabled,
  ...rest
}: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      disabled={disabled || loading}
      className={cn(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-lg whitespace-nowrap transition-colors",
        "disabled:pointer-events-none disabled:opacity-50",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3.5 text-[13px]",
        BUTTON_VARIANTS[variant],
        className,
      )}
    >
      {loading ? <Spinner className="size-3.5" /> : icon}
      {children}
    </button>
  );
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  active?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

/** Square icon-only button. The label is both the tooltip and the accessible name. */
export function IconButton({ label, active, className, children, ...rest }: IconButtonProps) {
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-label={label}
        {...rest}
        className={cn(
          "inline-flex size-7 shrink-0 items-center justify-center rounded-md transition-colors",
          "disabled:pointer-events-none disabled:opacity-40",
          active ? "bg-accent-soft text-accent-text" : "text-fg-muted hover:bg-sunken hover:text-fg",
          className,
        )}
      >
        {children}
      </button>
    </Tooltip>
  );
}

// ---- Feedback ----------------------------------------------------------------------

export function Spinner({ className }: { className?: string }) {
  return <LoaderCircle aria-hidden className={cn("size-4 animate-spin", className)} />;
}

export function Tooltip({ label, children }: { label: string; children: ReactNode }) {
  return (
    <RTooltip.Root>
      <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
      <RTooltip.Portal>
        <RTooltip.Content
          sideOffset={6}
          className="animate-fade z-[70] max-w-64 rounded-md bg-fg px-2 py-1 text-xs text-surface shadow-pop"
        >
          {label}
        </RTooltip.Content>
      </RTooltip.Portal>
    </RTooltip.Root>
  );
}

// ---- Form controls -----------------------------------------------------------------

export const inputClass = cn(
  "h-8 w-full rounded-lg border border-border-strong bg-surface px-2.5 text-[13px] text-fg",
  "placeholder:text-fg-subtle transition-[border-color,box-shadow]",
  "hover:border-fg-subtle focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent-soft",
  "disabled:opacity-50",
);

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement> & { ref?: Ref<HTMLInputElement> }) {
  return <input autoComplete="off" autoCorrect="off" autoCapitalize="off" spellCheck={false} {...rest} className={cn(inputClass, className)} />;
}

interface FieldProps {
  label: string;
  hint?: ReactNode;
  optional?: string;
  className?: string;
  children: ReactNode;
}

/** Label + control + hint. Wrapping in <label> ties the text to the control. */
export function Field({ label, hint, optional, className, children }: FieldProps) {
  return (
    <label className={cn("flex min-w-0 flex-col gap-1", className)}>
      <span className="flex items-baseline gap-1.5 text-xs font-medium text-fg-muted">
        {label}
        {optional && <span className="font-normal text-fg-subtle">· {optional}</span>}
      </span>
      {children}
      {hint && <span className="text-xs leading-snug text-fg-subtle">{hint}</span>}
    </label>
  );
}

interface SegmentedProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: { value: T; label: ReactNode; className?: string }[];
  label: string;
  className?: string;
}

export function Segmented<T extends string>({ value, onChange, options, label, className }: SegmentedProps<T>) {
  return (
    <div role="radiogroup" aria-label={label} className={cn("inline-flex rounded-lg bg-sunken p-0.5", className)}>
      {options.map((o) => {
        const selected = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => onChange(o.value)}
            className={cn(
              "inline-flex h-7 flex-1 items-center justify-center gap-1.5 rounded-md px-3 text-xs whitespace-nowrap transition-colors",
              selected ? "bg-surface font-semibold text-fg shadow-card ring-1 ring-border" : "text-fg-muted hover:text-fg",
              selected && o.className,
            )}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <RSwitch.Root
      checked={checked}
      onCheckedChange={onChange}
      aria-label={label}
      className="relative h-5 w-9 shrink-0 rounded-full bg-border-strong transition-colors data-[state=checked]:bg-accent"
    >
      <RSwitch.Thumb className="block size-4 translate-x-0.5 rounded-full bg-white shadow transition-transform data-[state=checked]:translate-x-[18px]" />
    </RSwitch.Root>
  );
}

// ---- Menus (shared look for context and dropdown menus) ----------------------------

export const menuContentClass =
  "animate-pop z-[60] min-w-48 rounded-xl border border-border bg-raised p-1 text-[13px] text-fg shadow-pop";
export const menuItemClass = cn(
  "flex h-7 cursor-default items-center gap-2 rounded-md px-2 outline-none select-none",
  "data-[highlighted]:bg-accent-soft data-[highlighted]:text-accent-text data-[disabled]:opacity-40",
);
export const menuDangerClass = "text-danger data-[highlighted]:bg-danger-soft data-[highlighted]:text-danger";
export const menuSeparatorClass = "my-1 h-px bg-border";

// ---- Empty / error states ----------------------------------------------------------

interface StateProps {
  icon: ReactNode;
  title: string;
  body?: ReactNode;
  action?: ReactNode;
  tone?: "neutral" | "danger";
}

export function StateMessage({ icon, title, body, action, tone = "neutral" }: StateProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      <div
        className={cn(
          "flex size-11 items-center justify-center rounded-2xl",
          tone === "danger" ? "bg-danger-soft text-danger" : "bg-accent-soft text-accent-text",
        )}
      >
        {icon}
      </div>
      <div className="max-w-md">
        <div className="text-sm font-semibold text-fg">{title}</div>
        {body && (
          <div data-selectable className="mt-1 text-[13px] leading-relaxed break-words text-fg-muted">
            {body}
          </div>
        )}
      </div>
      {action}
    </div>
  );
}
