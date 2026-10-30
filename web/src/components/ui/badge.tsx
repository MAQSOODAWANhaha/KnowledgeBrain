import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center gap-1.5 rounded-[4px] px-1.5 py-0.5 text-xs font-semibold",
  {
    variants: {
      tone: {
        sky: "bg-sky-wash text-sky-ink",
        gray: "bg-canvas text-quiet",
        go: "bg-[var(--pine-soft)] text-go",
        wait: "bg-[var(--amber-soft)] text-wait",
        stop: "bg-[var(--rose-soft)] text-stop",
      },
    },
    defaultVariants: { tone: "gray" },
  },
);

export function Badge({
  className,
  tone,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone, className }))} {...props} />;
}
