import { cn } from "@/lib/utils";

function Progress({
  value = 0,
  className,
  estimated = false,
}: {
  value?: number;
  className?: string;
  /** An estimate rather than reported progress: drawn lighter and striped. */
  estimated?: boolean;
}) {
  return (
    <div
      role="progressbar"
      aria-valuenow={value}
      aria-valuemin={0}
      aria-valuemax={100}
      title={estimated ? "Estimated progress" : undefined}
      className={cn(
        "relative h-1.5 w-full overflow-hidden rounded-full bg-primary/15",
        className,
      )}
    >
      <div
        className={cn(
          "h-full rounded-full bg-primary transition-[width] duration-300",
          estimated &&
            "bg-primary/50 [background-image:repeating-linear-gradient(45deg,transparent_0_4px,rgba(255,255,255,0.35)_4px_8px)]",
        )}
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  );
}
export { Progress };
