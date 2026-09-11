import type { ComponentProps, ReactNode } from "react";
import { Input } from "./input";
import { Label } from "./label";

export function Field({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={className}>
      <Label>{label}</Label>
      {children}
    </div>
  );
}

export function TextField({
  label,
  id,
  ...props
}: { label: string; id?: string } & ComponentProps<typeof Input>) {
  return (
    <Field label={label}>
      <Input id={id} {...props} />
    </Field>
  );
}
