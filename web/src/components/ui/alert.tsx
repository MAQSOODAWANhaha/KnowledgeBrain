import { cn } from "../../lib/utils";

export function Alert({
  className,
  tone = "stop",
  ...props
}: React.ComponentProps<"div"> & { tone?: "stop" | "go" | "wait" | "sky" }) {
  const tones = {
    stop: "border-[#fecaca] bg-[#fef2f2] text-stop",
    go: "border-[#a7f3d0] bg-[#ecfdf5] text-go",
    wait: "border-[#fed7aa] bg-[#fff7ed] text-wait",
    sky: "border-sky-wash bg-sky-wash text-sky-ink",
  };
  return (
    <div
      role="alert"
      className={cn("rounded-[8px] border px-3 py-2 text-sm", tones[tone], className)}
      {...props}
    />
  );
}
