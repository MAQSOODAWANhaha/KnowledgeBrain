import * as DialogPrimitive from "@radix-ui/react-dialog";
import { IconX } from "@tabler/icons-react";
import type { ComponentProps } from "react";
import { cn } from "../../lib/utils";

export const Sheet = DialogPrimitive.Root;
export const SheetClose = DialogPrimitive.Close;

export function SheetContent({
  className,
  children,
  title,
  ...props
}: ComponentProps<typeof DialogPrimitive.Content> & { title?: string }) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-ink/40" />
      <DialogPrimitive.Content
        className={cn(
          "fixed inset-y-0 right-0 z-50 flex w-[min(80vw,960px)] flex-col border-l border-line bg-white shadow-xl",
          className,
        )}
        {...props}
      >
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-line px-4">
          <DialogPrimitive.Title className="truncate text-sm font-semibold">
            {title ?? "预览"}
          </DialogPrimitive.Title>
          <DialogPrimitive.Close className="text-quiet hover:text-ink" aria-label="关闭">
            <IconX size={16} stroke={1.8} />
          </DialogPrimitive.Close>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden">{children}</div>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}
