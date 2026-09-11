import * as DialogPrimitive from "@radix-ui/react-dialog";
import { IconX } from "@tabler/icons-react";
import type { ComponentProps } from "react";
import { cn } from "../../lib/utils";

export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogClose = DialogPrimitive.Close;

export function DialogContent({
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
          "fixed left-1/2 top-1/2 z-50 w-[min(440px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 rounded-[14px] border border-line bg-white p-6 shadow-[0_8px_24px_rgba(17,17,26,.08),0_24px_64px_rgba(17,17,26,.10)]",
          className,
        )}
        {...props}
      >
        {title ? (
          <DialogPrimitive.Title className="mb-4 text-[16px] font-semibold text-ink">
            {title}
          </DialogPrimitive.Title>
        ) : (
          <DialogPrimitive.Title className="sr-only">对话框</DialogPrimitive.Title>
        )}
        {children}
        <DialogPrimitive.Close className="absolute right-4 top-4 text-quiet hover:text-ink" aria-label="关闭">
          <IconX size={16} stroke={1.8} />
        </DialogPrimitive.Close>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}
