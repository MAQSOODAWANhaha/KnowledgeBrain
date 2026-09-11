import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold",
  {
    variants: {
      tone: {
        sky: "bg-sky-wash text-sky-ink",
        gray: "bg-[#f1f5f9] text-quiet",
        go: "bg-[#ecfdf5] text-go",
        wait: "bg-[#fff7ed] text-wait",
        stop: "bg-[#fef2f2] text-stop",
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
