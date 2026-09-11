import { cn } from "../../lib/utils";

export function Progress({
  value = 0,
  className,
  tone = "sky",
  animated,
}: {
  value?: number;
  className?: string;
  tone?: "sky" | "go" | "stop";
  animated?: boolean;
}) {
  const bar = tone === "go" ? "bg-go" : tone === "stop" ? "bg-stop" : "bg-sky";
  return (
    <div className={cn("h-1.5 w-full overflow-hidden rounded-full bg-line", className)}>
      <div
        className={cn("h-full rounded-full", bar, animated && "animate-pulse")}
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  );
}
