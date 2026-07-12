import * as React from "react";
import { cn } from "@/lib/utils";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "flex w-full min-h-16 rounded-2xl border border-border bg-background px-4 py-2.5 text-sm font-sans text-foreground outline-none transition-colors placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 focus-visible:border-ring resize-none",
        className
      )}
      {...props}
    />
  );
}

export { Textarea };
