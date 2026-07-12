"use client";

import React from "react";
import { Dialog, DialogContent } from "@/components/ui/dialog";

export function ImageLightbox({
  src,
  open,
  onOpenChange,
}: {
  src: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[90vw] w-fit p-2 bg-transparent border-none shadow-none">
        {src && (
          // eslint-disable-next-line @next/next/no-img-element
          <img src={src} alt="" className="max-w-[86vw] max-h-[86vh] w-auto h-auto rounded-lg object-contain" />
        )}
      </DialogContent>
    </Dialog>
  );
}
