import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-[9px] text-[13.5px] font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-[.62] [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "bg-sky text-white hover:bg-sky-ink",
        outline: "border border-line bg-white text-ink hover:bg-canvas",
        ghost: "text-ink hover:bg-canvas",
        destructive: "text-stop hover:bg-[#fef1f3]",
      },
      size: {
        default: "h-9 px-4",
        sm: "h-8 px-3 text-[12.5px]",
        lg: "h-10 px-4",
        icon: "h-[34px] w-[34px]",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

export function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "button";
  return <Comp className={cn(buttonVariants({ variant, size, className }))} {...props} />;
}
