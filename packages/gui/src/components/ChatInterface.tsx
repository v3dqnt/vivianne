"use client";

import React, { useState, useEffect, useRef, memo, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion, AnimatePresence } from "motion/react";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  Tick02Icon,
  SentIcon,
  BotIcon,
  ArrowUp01Icon,
  SidebarLeftIcon,
  Cancel01Icon,
  InformationCircleIcon,
  ImageAdd02Icon,
  SparklesIcon,
} from "@hugeicons/core-free-icons";
import { useAgent } from "./AgentProvider";
import { WorkspacePanel } from "./WorkspacePanel";
import { ProviderIcon } from "./ProviderIcons";
import { TextGenerateEffect } from "./ui/text-generate-effect";
import { ToolCard } from "./ToolCards/ToolCard";
import { CopyButton } from "./CopyButton";
import { Message, MessageContent, MessageResponse } from "./ai-elements/message";
import { Reasoning, ReasoningTrigger, ReasoningContent } from "./ai-elements/reasoning";
import { SlashCommandMenu } from "./SlashCommandMenu";
import { SkillMentionMenu, type MentionableSkill } from "./SkillMentionMenu";
import { ActivityCluster, groupActivityRows, shouldCollapseCluster } from "./ActivityCluster";
import { ImageLightbox } from "./ImageLightbox";
import { PromptRail } from "./PromptRail";
import { filterSlashCommands, getSlashQuery, getSkillMentionQuery, applySkillMention, type SlashCommand } from "@/lib/slashCommands";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

const THINKING_LEVELS = ["off", "minimal", "low", "medium", "high"];
const PERSONA_MODES = ["agent", "ask", "debug", "explore"];

/** Small capability pills shown next to a model in the selector. Labels are
 *  best-effort (pattern-matched from the model id backend-side), so this is a
 *  hint, not a guarantee. */
function ModelCapabilityBadges({
  model,
}: {
  model: { isImageModel?: boolean; isAudioModel?: boolean; supportsVision?: boolean };
}) {
  const badges: { label: string; cls: string }[] = [];
  if (model.isImageModel) badges.push({ label: "IMAGE", cls: "bg-pink-500/12 text-pink-600" });
  if (model.isAudioModel) badges.push({ label: "AUDIO", cls: "bg-amber-500/12 text-amber-600" });
  if (model.supportsVision) badges.push({ label: "VISION", cls: "bg-sky-500/12 text-sky-600" });
  if (badges.length === 0) return null;
  return (
    <span className="flex items-center gap-1 flex-shrink-0">
      {badges.map((b) => (
        <span
          key={b.label}
          className={cn("px-1 py-px rounded text-[8px] font-bold tracking-wide leading-none", b.cls)}
        >
          {b.label}
        </span>
      ))}
    </span>
  );
}

interface AgentMessageLike {
  id: string;
  role: "user" | "assistant" | "system" | "toolResult" | "toolCall";
  content: string;
  thinking?: string;
  isStreaming?: boolean;
  toolName?: string;
  isError?: boolean;
  args?: any;
  details?: Record<string, any> | null;
  status?: "running" | "success" | "error" | "pending";
  images?: { base64: string; mediaType: string }[];
}

function MessageImages({ images }: { images: { base64: string; mediaType: string }[] }) {
  const [lightbox, setLightbox] = useState<string | null>(null);
  return (
    <div className="flex flex-wrap gap-2 mb-2">
      {images.map((img, i) => {
        const src = `data:${img.mediaType};base64,${img.base64}`;
        return (
          <img
            key={i}
            src={src}
            alt="attached"
            onClick={() => setLightbox(src)}
            className="h-28 w-auto rounded-lg border border-hairline object-cover cursor-zoom-in hover:opacity-90 transition-opacity"
          />
        );
      })}
      <ImageLightbox src={lightbox} open={!!lightbox} onOpenChange={(open) => !open && setLightbox(null)} />
    </div>
  );
}

// ── Activity Row (memoized so unrelated rows don't re-render on every streamed token) ──
const ActivityRow = memo(function ActivityRow({ msg }: { msg: AgentMessageLike }) {
  const isUser = msg.role === "user";
  const isTool = msg.role === "toolResult";

  if (isTool) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
        className="w-full"
      >
        <ToolCard
          toolName={msg.toolName}
          content={msg.content}
          args={msg.args}
          details={msg.details}
          status={msg.status}
          isStreaming={msg.isStreaming}
          isError={msg.isError}
          images={msg.images}
        />
      </motion.div>
    );
  }

  if (isUser) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
        className="w-full"
      >
        <Message from="user" className="max-w-full">
          <MessageContent className="group-[.is-user]:bg-surface-soft group-[.is-user]:text-ink group-[.is-user]:rounded-xl">
            {msg.images && msg.images.length > 0 && <MessageImages images={msg.images} />}
            <p className="text-[14px] leading-relaxed whitespace-pre-wrap font-sans">{msg.content}</p>
          </MessageContent>
        </Message>
      </motion.div>
    );
  }

  // Assistant text — plain prose, no bubble
  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
      className="w-full flex gap-3"
    >
      <div className="flex-shrink-0 w-6 h-6 rounded-full border border-hairline bg-surface-soft flex items-center justify-center text-ink mt-0.5">
        <HugeiconsIcon icon={BotIcon} size={12} />
      </div>
      <div className="flex-1 min-w-0 pt-0.5">
        {/* Thinking block — collapsible, auto-opens while streaming, auto-closes when done */}
        {msg.thinking && (
          <Reasoning isStreaming={msg.isStreaming && !msg.content} className="mb-2">
            <ReasoningTrigger className="text-[11px]" />
            <ReasoningContent className="text-[12px] pl-3 border-l-2 border-hairline max-h-64 overflow-y-auto">
              {msg.thinking}
            </ReasoningContent>
          </Reasoning>
        )}
        {/* Empty streaming state — show pulsing dots while waiting for first token */}
        {msg.isStreaming && !msg.content && !msg.thinking && (
          <div className="flex items-center gap-0.5 py-1">
            {[0, 0.2, 0.4].map((delay) => (
              <motion.span
                key={delay}
                className="inline-block w-1.5 h-1.5 rounded-full bg-mute"
                animate={{ opacity: [0.3, 1, 0.3] }}
                transition={{ duration: 1.1, repeat: Infinity, delay }}
              />
            ))}
          </div>
        )}
        <Message from="assistant" className="max-w-full">
          <MessageContent className="!p-0 !bg-transparent">
            <MessageResponse>{msg.content}</MessageResponse>
          </MessageContent>
        </Message>
        {msg.isStreaming && msg.content && (
          <span className="inline-block w-1.5 h-3.5 bg-ink animate-pulse ml-0.5 align-middle" />
        )}
      </div>
    </motion.div>
  );
});

interface ChatInterfaceProps {
  onToggleSidebar?: () => void;
  onOpenSettings?: () => void;
}

// ── Main Chat Interface ──
export function ChatInterface({ onToggleSidebar, onOpenSettings }: ChatInterfaceProps) {
  const { messages, sendMessage, state, availableModels, setModel, setAutoModelEnabled, setThinkingLevel, setPersonaMode, sessionStats, agentReset, abortAgent, isBusy, agentMode, setAgentMode, listSkills, authenticatedProviders } = useAgent();
  const hasNoApiKeys = authenticatedProviders.size === 0;
  const [mentionableSkills, setMentionableSkills] = useState<MentionableSkill[]>([]);
  const [mentionIndex, setMentionIndex] = useState(0);

  // Fetch once on mount — the default `~/.viv/skills` dir is always scanned
  // by cmd_list_skills regardless of the extra-paths argument, so `[]` still
  // picks up everything the Settings → Skills panel shows by default.
  useEffect(() => {
    listSkills([]).then((res) => {
      setMentionableSkills((res.skills ?? []).map((s: any) => ({ name: s.name, description: s.description })));
    });
  }, [listSkills]);
  const [inputValue, setInputValue] = useState("");
  const [activeCommand, setActiveCommand] = useState<SlashCommand | null>(null);
  const [slashIndex, setSlashIndex] = useState(0);
  const [modelQuery, setModelQuery] = useState("");
  const [infoOpen, setInfoOpen] = useState(false);
  const [attachments, setAttachments] = useState<{ id: string; name: string; base64: string; mediaType: string; previewUrl: string }[]>([]);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const isGenerating = isBusy || messages.some((m) => m.isStreaming);

  // ── Slash commands ──
  // Only trigger the menu when there is no active command chip yet.
  const slashQuery = activeCommand ? null : getSlashQuery(inputValue);
  const slashCommands = useMemo(
    () => (slashQuery === null ? [] : filterSlashCommands(slashQuery)),
    [slashQuery]
  );
  const slashOpen = slashCommands.length > 0;

  // ── @skill mentions ── mutually exclusive with the slash-command menu
  // (their trigger patterns can't both match at once), so no conflict
  // needing extra guard logic.
  const mentionQuery = getSkillMentionQuery(inputValue);
  const mentionMatches = useMemo(() => {
    if (mentionQuery === null) return [];
    const q = mentionQuery.toLowerCase();
    return mentionableSkills.filter((s) => s.name.toLowerCase().includes(q)).slice(0, 8);
  }, [mentionQuery, mentionableSkills]);
  const mentionOpen = mentionMatches.length > 0;

  useEffect(() => {
    setMentionIndex(0);
  }, [mentionQuery]);

  const applyMention = (skill: MentionableSkill) => {
    setInputValue((v) => applySkillMention(v, skill.name));
    focusInput();
  };

  const focusInput = () => {
    requestAnimationFrame(() => inputRef.current?.focus());
  };

  useEffect(() => {
    setSlashIndex(0);
  }, [slashQuery]);

  const runSlashCommand = (cmd: SlashCommand) => {
    if (cmd.kind === "compose") {
      // Turn the command into a highlighted prefix chip; the user then types
      // the instruction after it (e.g. /computer → "open terminal").
      setActiveCommand(cmd);
      setInputValue("");
      focusInput();
      return;
    }
    // kind: "action"
    setInputValue("");
    switch (cmd.name) {
      case "new":
        agentReset();
        break;
      case "settings":
      case "model":
      case "session":
      case "export":
      case "tree":
      case "resume":
      case "name":
      case "reload":
        onOpenSettings?.();
        break;
      case "copy": {
        const lastAssistant = [...messages].reverse().find((m) => m.role === "assistant" && m.content);
        if (lastAssistant) navigator.clipboard.writeText(lastAssistant.content);
        break;
      }
      default:
        break;
    }
  };

  // Auto-scroll only while the user is already at (or near) the bottom, and
  // jump instantly rather than smooth-scrolling — re-triggering a smooth
  // scroll animation on every streamed token is what caused the stutter.
  const stickToBottomRef = useRef(true);
  const scrollContainerRef = useRef<HTMLDivElement>(null);

  const handleScroll = () => {
    const el = scrollContainerRef.current;
    if (!el) return;
    stickToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };

  useEffect(() => {
    if (stickToBottomRef.current) {
      messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
    }
  }, [messages]);

  const handleSubmit = () => {
    if (isGenerating) return;
    const arg = inputValue.trim();
    if (activeCommand) {
      if (!arg) return;
      const message = activeCommand.buildMessage ? activeCommand.buildMessage(arg) : `/${activeCommand.name} ${arg}`;
      sendMessage(message);
      setActiveCommand(null);
      setInputValue("");
      if (inputRef.current) inputRef.current.style.height = "auto";
      return;
    }
    if (!arg && attachments.length === 0) return;
    sendMessage(arg, attachments.map((a) => ({ base64: a.base64, mediaType: a.mediaType })));
    setInputValue("");
    attachments.forEach((a) => URL.revokeObjectURL(a.previewUrl));
    setAttachments([]);
    if (inputRef.current) inputRef.current.style.height = "auto";
  };

  const handleFilesSelected = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = "";
    for (const file of files) {
      if (!file.type.startsWith("image/")) continue;
      const reader = new FileReader();
      reader.onload = () => {
        const dataUrl = reader.result as string;
        const base64 = dataUrl.split(",")[1] ?? "";
        setAttachments((prev) => [
          ...prev,
          {
            id: crypto.randomUUID(),
            name: file.name,
            base64,
            mediaType: file.type,
            previewUrl: URL.createObjectURL(file),
          },
        ]);
      };
      reader.readAsDataURL(file);
    }
  };


  const removeAttachment = (id: string) => {
    setAttachments((prev) => {
      const found = prev.find((a) => a.id === id);
      if (found) URL.revokeObjectURL(found.previewUrl);
      return prev.filter((a) => a.id !== id);
    });
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (mentionOpen) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMentionIndex((i) => (i + 1) % mentionMatches.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setMentionIndex((i) => (i - 1 + mentionMatches.length) % mentionMatches.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        applyMention(mentionMatches[mentionIndex]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setInputValue((v) => v.replace(/@[A-Za-z0-9_-]*$/, ""));
        return;
      }
    }
    if (slashOpen) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSlashIndex((i) => (i + 1) % slashCommands.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSlashIndex((i) => (i - 1 + slashCommands.length) % slashCommands.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        runSlashCommand(slashCommands[slashIndex]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setInputValue("");
        return;
      }
    }
    // Backspace at the very start of an empty input removes the command chip.
    if (e.key === "Backspace" && activeCommand && inputValue === "") {
      e.preventDefault();
      setActiveCommand(null);
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInputValue(e.target.value);
    e.target.style.height = "auto";
    e.target.style.height = Math.min(e.target.scrollHeight, 200) + "px";
  };

  const activeProvider = state.model?.provider ?? null;

  return (
    <div className="flex flex-col flex-1 h-full bg-transparent overflow-hidden select-none">
      {/* ── Top Bar — no border, floats over content ── */}
      <header className="flex items-center h-11 px-4 flex-shrink-0 z-10">
        <div className="flex items-center gap-2 min-w-0">
          {onToggleSidebar && (
            <Button variant="ghost" size="icon-sm" onClick={onToggleSidebar} aria-label="Toggle sidebar">
              <HugeiconsIcon icon={SidebarLeftIcon} size={15} />
            </Button>
          )}
          <span className="text-[13px] font-semibold tracking-tight text-ink truncate">
            {state.sessionName || "New session"}
          </span>
        </div>
        <div className="ml-auto flex items-center">
          <PromptRail
            messages={messages}
            onJump={(id) => {
              document.getElementById(`msg-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
            }}
          />
        </div>
      </header>

      {/* ── Activity Feed ── */}
      <div ref={scrollContainerRef} onScroll={handleScroll} className="flex-1 overflow-y-auto bg-transparent">
        {messages.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full px-6">
            <div className="max-w-lg text-center space-y-8">
              <TextGenerateEffect
                words="Build with AI, directly."
                className="text-3xl md:text-4xl font-bold font-sans text-ink tracking-tight leading-tight"
                duration={0.6}
              />
              <p className="text-body text-sm max-w-sm mx-auto font-sans leading-relaxed">
                Vivianne is your autonomous coding assistant. Prompt it to read files, write code, run commands, and execute tasks.
              </p>

              {hasNoApiKeys ? (
                <div className="flex flex-col items-center gap-3">
                  <p className="text-sm font-sans text-body max-w-xs mx-auto">
                    To get started, add an API key for a provider like Anthropic, OpenAI, or any of the others.
                  </p>
                  <Button onClick={() => onOpenSettings?.()} size="sm">
                    Add an API key
                  </Button>
                </div>
              ) : (
                <div className="bg-surface-soft rounded-full px-5 py-3 flex items-center gap-3 w-fit mx-auto border border-hairline">
                  <code className="font-mono text-xs text-ink">viv --model claude-sonnet</code>
                  <CopyButton text="viv --model claude-sonnet" />
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="max-w-3xl mx-auto px-4 md:px-6 py-6 md:py-8 space-y-3">
            <AnimatePresence initial={false}>
              {groupActivityRows(messages.filter((m) => m.role !== "system")).map((group, gIdx) => {
                if (group.kind === "single") {
                  const msg = group.items[0];
                  return (
                    <div key={msg.id || gIdx} id={`msg-${msg.id}`}>
                      <ActivityRow msg={msg} />
                    </div>
                  );
                }
                return (
                  <div key={`cluster-${gIdx}`} id={`msg-${group.items[0].id}`}>
                    <ActivityCluster count={group.items.length} defaultCollapsed={shouldCollapseCluster(group.items.length)}>
                      {group.items.map((msg, i) => (
                        <ActivityRow key={msg.id || `${gIdx}-${i}`} msg={msg} />
                      ))}
                    </ActivityCluster>
                  </div>
                );
              })}
            </AnimatePresence>
            <div ref={messagesEndRef} />
          </div>
        )}
      </div>

      {/* ── Composer — floats above the feed, doesn't span full width ── */}
      <div className="flex-shrink-0 px-4 md:px-6 pb-4 pt-1">
        <div className="max-w-2xl mx-auto relative">
          {/* Context usage popover — rendered outside the clipped composer well */}
          <AnimatePresence>
            {infoOpen && (
              <motion.div
                initial={{ opacity: 0, y: 4, scale: 0.97 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 4, scale: 0.97 }}
                transition={{ duration: 0.12 }}
                className="absolute bottom-full right-0 mb-2 w-56 rounded-xl bg-canvas border border-hairline shadow-lg p-3 z-50 select-text"
              >
                <p className="text-[11px] font-semibold text-ink mb-2">Context</p>
                <div className="space-y-1.5 text-[11px] font-mono text-body">
                  {sessionStats ? (
                    <>
                      <div className="flex justify-between">
                        <span className="text-mute">Tokens used</span>
                        <span className="text-ink">{sessionStats.totalTokens.toLocaleString()}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-mute">Messages</span>
                        <span className="text-ink">{sessionStats.messageCount}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-mute">Context used</span>
                        <span className="text-ink">{Math.round(sessionStats.contextPercent)}%</span>
                      </div>
                      {state.model?.contextWindow && (
                        <div className="flex justify-between">
                          <span className="text-mute">Window</span>
                          <span className="text-ink">
                            {state.model.contextWindow >= 1000
                              ? `${Math.round(state.model.contextWindow / 1000)}K`
                              : state.model.contextWindow}
                          </span>
                        </div>
                      )}
                      {/* Usage bar */}
                      <div className="mt-2 h-1 rounded-full bg-surface-soft overflow-hidden">
                        <div
                          className="h-full rounded-full bg-ink/40 transition-all"
                          style={{ width: `${Math.min(sessionStats.contextPercent, 100)}%` }}
                        />
                      </div>
                    </>
                  ) : (
                    <span className="text-mute">No active session</span>
                  )}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
          {/* Slash menu — anchored to the composer (outside the clipped well) */}
          <AnimatePresence>
            {slashOpen && (
              <SlashCommandMenu
                commands={slashCommands}
                activeIndex={slashIndex}
                onSelect={runSlashCommand}
                onHover={setSlashIndex}
              />
            )}
          </AnimatePresence>
          {/* @skill mention menu — same anchor point as the slash menu */}
          <AnimatePresence>
            {mentionOpen && (
              <SkillMentionMenu
                skills={mentionMatches}
                activeIndex={mentionIndex}
                onSelect={applyMention}
                onHover={setMentionIndex}
              />
            )}
          </AnimatePresence>
          <div className="rounded-2xl overflow-hidden bg-canvas border border-hairline shadow-sm">
            {/* Row 1 — quick controls */}
            <div className="flex items-center gap-1.5 px-2.5 pt-2.5">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className={cn(
                      "flex items-center gap-1.5 px-2.5 py-1 backdrop-blur-sm hover:bg-accent text-[11px] font-semibold rounded-full border transition-colors select-none",
                      state.autoModelEnabled
                        ? "bg-violet-500/10 border-violet-500/30 text-violet-700"
                        : "bg-canvas/50 border-hairline/60 text-ink"
                    )}
                  >
                    {state.autoModelEnabled ? (
                      <HugeiconsIcon icon={SparklesIcon} size={12} />
                    ) : (
                      activeProvider && <ProviderIcon provider={activeProvider} size={12} />
                    )}
                    <span>{state.autoModelEnabled ? "Auto" : activeProvider ? state.model?.name : "Select Model"}</span>
                    <HugeiconsIcon icon={ArrowUp01Icon} size={9} className="text-mute" />
                  </button>
                </DropdownMenuTrigger>

                <DropdownMenuContent
                  align="start"
                  side="top"
                  className="w-72 max-h-[60vh] overflow-hidden flex flex-col"
                  onCloseAutoFocus={() => setModelQuery("")}
                  {...({ onOpenAutoFocus: (e: Event) => e.preventDefault() } as any)}
                >
                  <DropdownMenuLabel>Switch Model</DropdownMenuLabel>
                  <DropdownMenuItem
                    onClick={() => setAutoModelEnabled(true)}
                    className={cn(state.autoModelEnabled && "bg-accent font-semibold")}
                  >
                    <HugeiconsIcon icon={SparklesIcon} size={13} className="flex-shrink-0 text-violet-600" />
                    <span className="flex-1">Auto — pick best model per turn</span>
                    {state.autoModelEnabled && <HugeiconsIcon icon={Tick02Icon} size={13} />}
                  </DropdownMenuItem>
                  {availableModels.length === 0 ? (
                    <div className="px-3.5 py-4 text-center text-xs text-mute">No authenticated models.</div>
                  ) : (
                    <>
                      <div className="px-2 pb-1.5 sticky top-0">
                        <input
                          autoFocus
                          value={modelQuery}
                          onChange={(e) => setModelQuery(e.target.value)}
                          // Stop Radix's menu typeahead / arrow-nav from stealing
                          // keystrokes (capture phase runs before Radix's handler).
                          onKeyDownCapture={(e) => e.stopPropagation()}
                          onKeyDown={(e) => e.stopPropagation()}
                          placeholder="Search models…"
                          className="w-full px-2.5 py-1.5 text-[12px] font-sans rounded-lg bg-surface-soft border border-hairline focus:outline-none focus:border-hairline-strong"
                        />
                      </div>
                      <div className="overflow-y-auto flex-1">
                        {(() => {
                          const q = modelQuery.trim().toLowerCase();
                          const filtered = q
                            ? availableModels.filter(
                                (m) =>
                                  m.name.toLowerCase().includes(q) ||
                                  m.provider.toLowerCase().includes(q) ||
                                  m.id.toLowerCase().includes(q)
                              )
                            : availableModels;
                          if (filtered.length === 0) {
                            return <div className="px-3.5 py-3 text-center text-xs text-mute">No matches.</div>;
                          }
                          return filtered.map((model) => {
                            const isSelected =
                              !state.autoModelEnabled && state.model?.id === model.id && state.model?.provider === model.provider;
                            return (
                              <DropdownMenuItem
                                key={`${model.provider}-${model.id}`}
                                onClick={() => {
                                  setModel(model.provider, model.id);
                                  setModelQuery("");
                                }}
                                className={cn(isSelected && "bg-accent font-semibold")}
                              >
                                <ProviderIcon provider={model.provider} size={13} className="flex-shrink-0" />
                                <span className="truncate flex-1">{model.name}</span>
                                <ModelCapabilityBadges model={model} />
                                {isSelected && <HugeiconsIcon icon={Tick02Icon} size={13} />}
                              </DropdownMenuItem>
                            );
                          });
                        })()}
                      </div>
                    </>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>

              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button className="flex items-center gap-1.5 px-2.5 py-1 bg-canvas hover:bg-accent text-[11px] font-semibold text-ink rounded-full border border-hairline transition-colors select-none capitalize">
                    <span>{state.thinkingLevel}</span>
                    <HugeiconsIcon icon={ArrowUp01Icon} size={9} className="text-mute" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" side="top" className="w-40">
                  <DropdownMenuLabel>Thinking Level</DropdownMenuLabel>
                  {THINKING_LEVELS.map((level) => (
                    <DropdownMenuItem
                      key={level}
                      onClick={() => setThinkingLevel(level)}
                      className={cn("capitalize", state.thinkingLevel === level && "bg-accent font-semibold")}
                    >
                      <span className="flex-1">{level}</span>
                      {state.thinkingLevel === level && <HugeiconsIcon icon={Tick02Icon} size={13} />}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>

              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button className="flex items-center gap-1.5 px-2.5 py-1 bg-canvas hover:bg-accent text-[11px] font-semibold text-ink rounded-full border border-hairline transition-colors select-none capitalize">
                    <span>{state.personaMode}</span>
                    <HugeiconsIcon icon={ArrowUp01Icon} size={9} className="text-mute" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" side="top" className="w-40">
                  <DropdownMenuLabel>Persona</DropdownMenuLabel>
                  {PERSONA_MODES.map((mode) => (
                    <DropdownMenuItem
                      key={mode}
                      onClick={() => setPersonaMode(mode)}
                      className={cn("capitalize", state.personaMode === mode && "bg-accent font-semibold")}
                    >
                      <span className="flex-1">{mode}</span>
                      {state.personaMode === mode && <HugeiconsIcon icon={Tick02Icon} size={13} />}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>

              {/* Mode toggle */}
              <button
                onClick={() => setAgentMode(agentMode === "auto" ? "permission" : "auto")}
                title={agentMode === "auto" ? "Auto mode — click for Permission mode" : "Permission mode — click for Auto mode"}
                className={cn(
                  "text-[10px] font-semibold px-2.5 py-1 rounded-full border transition-colors select-none",
                  agentMode === "auto"
                    ? "bg-amber-500/10 border-amber-500/30 text-amber-700 hover:bg-amber-500/15"
                    : "bg-surface-soft border-hairline text-body hover:bg-accent"
                )}
              >
                {agentMode === "auto" ? "AUTO" : "PERM"}
              </button>

              {/* Consolidated workspace control — folders (multi), create/add/remove,
                  and the read-only toggle all live in one popover now instead of
                  two separate composer buttons (see WorkspacePanel.tsx). */}
              <WorkspacePanel />

              {/* Screenshot button removed as per request */}

              {/* Attach image */}
              <button
                onClick={() => fileInputRef.current?.click()}
                title="Attach image"
                className="w-7 h-7 flex items-center justify-center rounded-full text-mute hover:text-ink hover:bg-surface-soft transition-colors"
              >
                <HugeiconsIcon icon={ImageAdd02Icon} size={14} />
              </button>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                className="hidden"
                onChange={handleFilesSelected}
              />

              {/* Info icon — context stats popover (panel rendered outside the clipped composer, see below) */}
              <button
                onClick={() => setInfoOpen((v) => !v)}
                className="ml-auto w-7 h-7 flex items-center justify-center rounded-full text-mute hover:text-ink hover:bg-surface-soft transition-colors"
                title="Context usage"
              >
                <HugeiconsIcon icon={InformationCircleIcon} size={14} />
              </button>
            </div>

            {/* Attachment thumbnails */}
            {attachments.length > 0 && (
              <div className="flex items-center gap-1.5 px-2.5 pt-2 flex-wrap">
                {attachments.map((a) => (
                  <div key={a.id} className="relative w-12 h-12 rounded-lg overflow-hidden border border-hairline group">
                    <img src={a.previewUrl} alt={a.name} className="w-full h-full object-cover" />
                    <button
                      onClick={() => removeAttachment(a.id)}
                      title="Remove"
                      className="absolute top-0 right-0 w-4 h-4 flex items-center justify-center bg-ink/70 text-on-primary opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      <HugeiconsIcon icon={Cancel01Icon} size={9} />
                    </button>
                  </div>
                ))}
              </div>
            )}

            {/* Row 2 — input */}
            <div className="flex items-end gap-2 p-2.5">
              {activeCommand && (
                <button
                  type="button"
                  onClick={() => {
                    setActiveCommand(null);
                    focusInput();
                  }}
                  title="Remove command"
                  className="flex-shrink-0 self-center flex items-center gap-1 pl-2.5 pr-2 py-1 rounded-full bg-ink text-on-primary text-[12px] font-mono font-semibold transition-opacity hover:opacity-90"
                >
                  <HugeiconsIcon icon={activeCommand.icon} size={12} />
                  <span>/{activeCommand.name}</span>
                  <HugeiconsIcon icon={Cancel01Icon} size={11} className="opacity-70" />
                </button>
              )}
              <Textarea
                ref={inputRef}
                value={inputValue}
                onChange={handleInput}
                onKeyDown={handleKeyDown}
                placeholder={
                  isGenerating
                    ? "Agent is working..."
                    : activeCommand
                      ? activeCommand.argHint ?? "Add your instruction…"
                      : "Ask anything, or type / for commands..."
                }
                rows={1}
                disabled={isGenerating}
                className="flex-1 bg-transparent border-none px-2 py-1.5 min-h-[36px] max-h-[200px] self-center focus-visible:border-transparent"
                style={{ height: "auto" }}
              />
              <Button
                onClick={isGenerating ? abortAgent : handleSubmit}
                disabled={!isGenerating && !inputValue.trim() && attachments.length === 0}
                size="icon"
                className="flex-shrink-0 self-center rounded-full"
                title={isGenerating ? "Stop" : "Send"}
              >
                <HugeiconsIcon icon={isGenerating ? Cancel01Icon : SentIcon} size={15} />
              </Button>
            </div>

          </div>
          <p className="text-center text-[10px] text-mute mt-2 font-sans">
            Vivianne can make mistakes. Please verify important code changes.
          </p>
        </div>
      </div>
    </div>
  );
}
