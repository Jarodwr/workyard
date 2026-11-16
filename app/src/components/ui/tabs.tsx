import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "@/lib/utils";

export type TabItem<T extends string> = {
  id: T;
  label: string;
  /** Badge shown after the label: a count, or a dot when count is 0. */
  badge?: { count: number; label: string } | null;
};

export const tabId = (prefix: string, id: string) => `${prefix}-tab-${id}`;
export const panelId = (prefix: string, id: string) => `${prefix}-panel-${id}`;

/** An accessible tab strip (roving tabindex, arrow/Home/End keys). Panels are rendered by the caller. */
export function TabList<T extends string>({
  items,
  value,
  onChange,
  prefix,
  label,
  next,
  className,
}: {
  items: TabItem<T>[];
  value: T;
  onChange: (id: T) => void;
  prefix: string;
  label: string;
  next: (current: T, key: string) => T | null;
  className?: string;
}) {
  const refs = useRef(new Map<string, HTMLButtonElement>());
  const onKeyDown = (event: KeyboardEvent) => {
    const target = next(value, event.key);
    if (!target) return;
    event.preventDefault();
    onChange(target);
    refs.current.get(target)?.focus();
  };
  return (
    <div
      role="tablist"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={cn("flex gap-1 overflow-x-auto border-b px-3 [scrollbar-width:none]", className)}
    >
      {items.map((item) => {
        const selected = item.id === value;
        return (
          <button
            key={item.id}
            ref={(node) => {
              if (node) refs.current.set(item.id, node);
              else refs.current.delete(item.id);
            }}
            type="button"
            role="tab"
            id={tabId(prefix, item.id)}
            aria-selected={selected}
            aria-controls={panelId(prefix, item.id)}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(item.id)}
            className={cn(
              "-mb-px inline-flex shrink-0 items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
              selected
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground",
            )}
          >
            {item.label}
            {item.badge && (
              <span
                aria-label={item.badge.label}
                title={item.badge.label}
                className={cn(
                  "inline-flex items-center justify-center rounded-full bg-primary text-[10px] font-semibold leading-none text-primary-foreground",
                  item.badge.count > 0 ? "min-w-4 px-1 py-0.5" : "size-1.5",
                )}
              >
                {item.badge.count > 0 ? item.badge.count : null}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

export function TabPanel({
  prefix,
  id,
  active,
  children,
  className,
}: {
  prefix: string;
  id: string;
  active: boolean;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      role="tabpanel"
      id={panelId(prefix, id)}
      aria-labelledby={tabId(prefix, id)}
      hidden={!active}
      tabIndex={0}
      className={cn("min-h-0 flex-1 overflow-y-auto px-5 py-4 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring", className)}
    >
      {active ? children : null}
    </div>
  );
}
