import { cn } from "../../lib/utils";

export function Alert({
  className,
  tone = "stop",
  ...props
}: React.ComponentProps<"div"> & { tone?: "stop" | "go" | "wait" | "sky" }) {
  const tones = {
    stop: "border-line bg-[var(--rose-soft)] text-stop",
    go: "border-line bg-[var(--pine-soft)] text-go",
    wait: "border-line bg-[var(--amber-soft)] text-wait",
    sky: "border-line bg-sky-wash text-sky-ink",
  };
  return (
    <div
      role="alert"
      className={cn("rounded-[6px] border px-3 py-2 text-[13px]", tones[tone], className)}
      {...props}
    />
  );
}
