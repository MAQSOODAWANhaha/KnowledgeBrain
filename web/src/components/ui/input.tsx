import * as React from "react";
import { cn } from "../../lib/utils";

export function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      className={cn(
        "flex h-8 w-full rounded-[6px] border border-input-line bg-white px-3 text-[13px] text-ink placeholder:text-quiet transition-colors duration-[140ms] ease-out focus-visible:outline-none focus-visible:border-sky focus-visible:ring-2 focus-visible:ring-sky focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-[.62]",
        className,
      )}
      {...props}
    />
  );
}
