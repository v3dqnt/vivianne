"use client";

import React, { useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { Copy01Icon, Tick02Icon } from "@hugeicons/core-free-icons";

export function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <button
      onClick={handleCopy}
      className="p-1 rounded-full hover:bg-surface-soft transition-colors text-body hover:text-ink flex items-center justify-center"
      aria-label="Copy"
    >
      <HugeiconsIcon icon={copied ? Tick02Icon : Copy01Icon} size={13} />
    </button>
  );
}
