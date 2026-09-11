import * as React from "react";
import { cn } from "../../lib/utils";

export function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      className={cn(
        "flex h-9 w-full rounded-[9px] border border-input-line bg-white px-3 text-sm text-ink placeholder:text-quiet focus-visible:outline-none focus-visible:border-sky focus-visible:ring-2 focus-visible:ring-sky focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-[.62]",
        className,
      )}
      {...props}
    />
  );
}
