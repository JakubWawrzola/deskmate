import type { ReactNode } from "react";

export function Panel({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) {
  return (
    <section className="bg-panel border border-hairline rounded-md">
      <header className="flex items-center justify-between px-4 h-10 border-b border-hairline">
        <h2 className="microlabel">{title}</h2>
        {action}
      </header>
      <div className="p-4">{children}</div>
    </section>
  );
}

export function Field({
  label,
  value,
  onChange,
  type = "text",
  placeholder,
  hint,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  placeholder?: string;
  hint?: string;
}) {
  return (
    <label className="block">
      <span className="microlabel">{label}</span>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        className="mt-1 w-full h-9 px-3 bg-panel border border-hairline-strong rounded
                   text-ink mono text-[13px] placeholder:text-faint
                   focus-visible:border-ink"
      />
      {hint && <span className="block mt-1 text-[12px] text-muted">{hint}</span>}
    </label>
  );
}

export function Toggle({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={`relative w-9 h-5 rounded-full border transition-colors duration-150
        ${on ? "bg-ink border-ink" : "bg-panel border-hairline-strong"}
        ${disabled ? "opacity-40" : "cursor-pointer"}`}
    >
      <span
        className={`absolute top-[3px] w-3 h-3 rounded-full transition-transform duration-150
          ${on ? "bg-panel translate-x-[19px]" : "bg-ink translate-x-[3px]"}`}
      />
    </button>
  );
}

export function Button({
  children,
  onClick,
  kind = "ghost",
  disabled,
}: {
  children: ReactNode;
  onClick?: () => void;
  kind?: "primary" | "ghost" | "danger";
  disabled?: boolean;
}) {
  const styles =
    kind === "primary"
      ? "bg-ink text-panel border-ink hover:bg-black"
      : kind === "danger"
        ? "bg-panel text-ink border-hairline-strong hover:border-ink"
        : "bg-panel text-ink border-hairline-strong hover:border-ink";
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`h-8 px-3 rounded border text-[13px] font-medium transition-colors duration-100
        disabled:opacity-40 ${styles}`}
    >
      {children}
    </button>
  );
}

/** Kropka statusu: wypelniona = polaczony, pusta = nie (monochrom zamiast koloru). */
export function StatusDot({ on }: { on: boolean }) {
  return (
    <span
      aria-hidden
      className={`inline-block w-2.5 h-2.5 rounded-full border border-ink
        ${on ? "bg-ink" : "bg-transparent"}`}
    />
  );
}

export function EmptyState({ text }: { text: string }) {
  return <p className="text-muted text-[13px] py-6 text-center">{text}</p>;
}
