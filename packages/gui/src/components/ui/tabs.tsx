"use client";

import * as React from "react";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import { motion } from "motion/react";
import { cn } from "@/lib/utils";

function Tabs({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Root>) {
  return <TabsPrimitive.Root data-slot="tabs" className={cn("flex flex-col", className)} {...props} />;
}

function TabsList({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn("inline-flex items-center gap-1", className)}
      {...props}
    />
  );
}

function TabsTrigger({
  className,
  children,
  value,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  const [isActive, setIsActive] = React.useState(false);
  const ref = React.useRef<HTMLButtonElement>(null);

  React.useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => setIsActive(el.getAttribute("data-state") === "active");
    update();
    const observer = new MutationObserver(update);
    observer.observe(el, { attributes: true, attributeFilter: ["data-state"] });
    return () => observer.disconnect();
  }, []);

  return (
    <TabsPrimitive.Trigger
      ref={ref}
      value={value}
      data-slot="tabs-trigger"
      className={cn(
        "relative px-4 py-2 text-sm font-medium font-sans rounded-full transition-colors outline-none",
        "text-muted-foreground hover:text-foreground data-[state=active]:text-on-primary",
        className
      )}
      {...props}
    >
      {isActive && (
        <motion.span
          layoutId="tabs-active-pill"
          className="absolute inset-0 bg-primary rounded-full -z-10"
          transition={{ type: "spring", bounce: 0.2, duration: 0.4 }}
        />
      )}
      <span className="relative z-10">{children}</span>
    </TabsPrimitive.Trigger>
  );
}

function TabsContent({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Content>) {
  return <TabsPrimitive.Content data-slot="tabs-content" className={cn("outline-none", className)} {...props} />;
}

export { Tabs, TabsList, TabsTrigger, TabsContent };
