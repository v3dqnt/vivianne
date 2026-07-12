"use client";

import React, { useState, memo } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  ArrowDown01Icon,
  ArrowRight01Icon,
  Copy01Icon,
  Tick02Icon,
  Loading03Icon,
  AlertCircleIcon,
} from "@hugeicons/core-free-icons";
import { cn } from "@/lib/utils";
import { ToolArgs } from "../types";
import { getToolMeta, summarizeArgs, toneFor, statusColor } from "./toolMeta";
import { Terminal, TerminalHeader, TerminalTitle, TerminalContent } from "../ai-elements/terminal";
import { Tool, ToolContent } from "../ai-elements/tool";
import { Agent, AgentHeader, AgentContent, AgentInstructions } from "../ai-elements/agent";
import { CollapsibleTrigger } from "@/components/ui/collapsible";
import { MessageResponse } from "../ai-elements/message";
import { DiffView } from "../DiffView";
import { ImageLightbox } from "../ImageLightbox";

function CopyButton({ text }: { text: string }) {
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
      className="p-1 rounded-md hover:bg-surface-soft transition-colors text-mute hover:text-ink"
      aria-label="Copy"
    >
      <HugeiconsIcon icon={copied ? Tick02Icon : Copy01Icon} size={12} />
    </button>
  );
}

function BashResult({ content, isError, isStreaming }: { content: string; isError?: boolean; isStreaming?: boolean }) {
  const lines = content.split("\n");
  const commandLine = lines[0]?.startsWith("$") ? lines[0] : null;
  const output = commandLine ? lines.slice(1).join("\n") : content;

  return (
    <Terminal
      output={output || (commandLine ? "" : content)}
      isStreaming={isStreaming}
      className="rounded-none border-none"
    >
      {commandLine && (
        <TerminalHeader className="border-zinc-800/80 py-1.5">
          <TerminalTitle className="text-emerald-400">
            {commandLine.replace(/^\$\s*/, "")}
          </TerminalTitle>
        </TerminalHeader>
      )}
      <TerminalContent
        className={cn("max-h-[320px] text-[11px]", isError && "text-rose-400")}
      />
    </Terminal>
  );
}

function FilePathResult({
  toolName,
  content,
  args,
  details,
  isError,
}: {
  toolName?: string;
  content: string;
  args?: ToolArgs | null;
  details?: Record<string, any> | null;
  isError?: boolean;
}) {
  const tool = (toolName ?? "").toLowerCase();
  const isEdit = tool === "edit";
  const isWrite = tool === "write";

  // edit: old_string/new_string are the exact before/after (the tool does an
  // exact-string replace, so this is a real pre/post image, not a guess).
  // write: the backend now captures the file's prior content (if any) into
  // details.previous_content before overwriting — undefined means a new
  // file, which correctly renders as an all-green "added" diff.
  const showDiff = (isEdit && typeof args?.old_string === "string") || (isWrite && typeof args?.content === "string");
  const oldText = isEdit ? args?.old_string : (details?.previous_content as string | undefined);
  const newText = isEdit ? (args?.new_string ?? "") : ((args?.content as string | undefined) ?? "");

  return (
    <div className="text-xs bg-surface-subtle">
      {showDiff && <DiffView oldText={oldText} newText={newText} />}
      <pre
        className={cn(
          "p-3.5 overflow-x-auto whitespace-pre-wrap leading-relaxed font-mono max-h-[280px] overflow-y-auto",
          isError ? "text-rose-700" : "text-body",
        )}
      >
        {content}
      </pre>
    </div>
  );
}

function DefaultResult({ content, isError, isStreaming }: { content: string; isError?: boolean; isStreaming?: boolean }) {
  return (
    <pre
      className={cn(
        "p-3.5 text-xs overflow-x-auto whitespace-pre-wrap leading-relaxed font-mono max-h-[320px] overflow-y-auto bg-surface-subtle",
        isError ? "text-rose-700" : "text-body",
      )}
    >
      {content}
      {isStreaming && <span className="inline-block w-1.5 h-3.5 bg-mute animate-pulse ml-1 align-middle" />}
    </pre>
  );
}

function DelegateResult({
  task,
  content,
  isStreaming,
}: {
  task?: string;
  content: string;
  isStreaming?: boolean;
}) {
  return (
    <Agent className="border-none rounded-none">
      <AgentHeader name="Subagent" />
      <AgentContent className="p-3.5 pt-0 space-y-3">
        {task && <AgentInstructions>{task}</AgentInstructions>}
        {content ? (
          <div className="space-y-1.5">
            <span className="font-medium text-mute text-xs">Result</span>
            <div className="rounded-md bg-surface-subtle p-3 text-body text-sm">
              <MessageResponse>{content}</MessageResponse>
            </div>
          </div>
        ) : isStreaming ? (
          <div className="flex items-center gap-2 text-body text-xs">
            <HugeiconsIcon icon={Loading03Icon} size={13} className="animate-spin" />
            Subagent working…
          </div>
        ) : null}
      </AgentContent>
    </Agent>
  );
}

export interface ToolCardProps {
  toolName?: string;
  content: string;
  args?: ToolArgs | null;
  details?: Record<string, any> | null;
  status?: "running" | "success" | "error" | "pending";
  isStreaming?: boolean;
  isError?: boolean;
  images?: { base64: string; mediaType: string }[];
}

function ToolCardImpl({ toolName, content, args, details, status, isStreaming, isError, images }: ToolCardProps) {
  const [open, setOpen] = useState(true);
  const [lightbox, setLightbox] = useState<string | null>(null);
  const meta = getToolMeta(toolName);
  const tone = toneFor(status ?? (isStreaming ? "running" : isError ? "error" : "success"));
  const colors = statusColor(tone);
  const summary = summarizeArgs(toolName, args);
  const toolKey = (toolName ?? "").toLowerCase();

  const renderResult = () => {
    if (toolKey === "delegate") {
      return <DelegateResult task={args?.task} content={content} isStreaming={isStreaming} />;
    }
    if (images && images.length > 0) {
      return (
        <div className="p-3.5 bg-surface-subtle space-y-2.5">
          <div className="flex flex-wrap gap-2.5">
            {images.map((img, i) => {
              const src = `data:${img.mediaType};base64,${img.base64}`;
              return (
                <img
                  key={i}
                  src={src}
                  onClick={() => setLightbox(src)}
                  alt={toolKey === "generate_image" ? "generated" : "screenshot"}
                  className="max-h-80 w-auto rounded-lg border border-hairline object-contain cursor-zoom-in hover:opacity-90 transition-opacity"
                />
              );
            })}
          </div>
          {content && <p className="text-xs text-body whitespace-pre-wrap">{content}</p>}
        </div>
      );
    }
    if (!content && isStreaming) {
      return (
        <div className="flex items-center gap-2 p-3.5 text-body text-xs bg-surface-subtle">
          <HugeiconsIcon icon={Loading03Icon} size={13} className="animate-spin" />
          Executing…
        </div>
      );
    }
    if (toolKey === "bash") return <BashResult content={content} isError={isError} isStreaming={isStreaming} />;
    if (["read", "write", "edit", "grep", "find", "ls"].includes(toolKey)) {
      return <FilePathResult toolName={toolName} content={content} args={args} details={details} isError={isError} />;
    }
    return <DefaultResult content={content} isError={isError} isStreaming={isStreaming} />;
  };

  return (
    <Tool
      open={open}
      onOpenChange={setOpen}
      className="w-full rounded-xl border border-hairline overflow-hidden my-1.5 mb-0 bg-canvas"
    >
      {/* asChild + <div role="button"> rather than Radix's default <button> trigger,
          since the header contains CopyButton's own <button> — nested <button>s are
          invalid HTML and React (correctly) errors on it during hydration. */}
      <CollapsibleTrigger asChild>
        <div
          role="button"
          tabIndex={0}
          className="w-full flex items-center gap-2.5 px-3.5 py-2.5 hover:bg-surface-soft transition-colors text-left cursor-pointer select-none"
        >
          <HugeiconsIcon icon={meta.icon} size={14} strokeWidth={2} className={cn("flex-shrink-0", meta.accent)} />
          <span className={cn("text-[13px] font-semibold font-sans flex-shrink-0", meta.accent)}>{meta.label}</span>
          {summary && (
            <span className="text-[12px] font-mono text-body truncate min-w-0 flex-1">{summary}</span>
          )}
          <div className="flex items-center gap-1.5 flex-shrink-0 ml-auto pl-2">
            {tone === "running" && <HugeiconsIcon icon={Loading03Icon} size={13} className="text-sky-600 animate-spin" />}
            {tone === "error" && <HugeiconsIcon icon={AlertCircleIcon} size={13} className="text-rose-600" />}
            {content && <CopyButton text={content} />}
            <HugeiconsIcon icon={open ? ArrowDown01Icon : ArrowRight01Icon} size={12} className="text-mute" />
          </div>
        </div>
      </CollapsibleTrigger>

      <ToolContent className="p-0 space-y-0 border-t border-hairline">
        {renderResult()}
      </ToolContent>
      <ImageLightbox src={lightbox} open={!!lightbox} onOpenChange={(open) => !open && setLightbox(null)} />
    </Tool>
  );
}

export const ToolCard = memo(ToolCardImpl);
