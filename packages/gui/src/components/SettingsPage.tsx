"use client";

import React, { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAgent } from "./AgentProvider";
import { motion, AnimatePresence } from "motion/react";
import { HugeiconsIcon } from "@hugeicons/react";
import type { IconSvgElement } from "@hugeicons/react";
import {
  PlusSignIcon,
  Delete02Icon,
  Alert02Icon,
  SidebarLeftIcon,
  Key01Icon,
  Database02Icon,
  NeuralNetworkIcon,
  PlayCircle02Icon,
  MagicWand01Icon,
  ToolsIcon,
  Upload01Icon,
  McpServerIcon,
  InformationCircleIcon,
  TestTube02Icon,
  Cancel01Icon,
  AlarmClockIcon,
  PlayIcon,
  Chart01Icon,
} from "@hugeicons/core-free-icons";
import { cn } from "@/lib/utils";
import { ApiKeyManager } from "./ApiKeyManager";
import { UsageSettings } from "./UsageSettings";
import { UpdateChecker } from "./UpdateChecker";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";

interface NavSection {
  id: string;
  label: string;
  icon: IconSvgElement;
}

const NAV_SECTIONS: NavSection[] = [
  { id: "api-keys", label: "API Keys", icon: Key01Icon },
  { id: "memory", label: "Memory", icon: Database02Icon },
  { id: "engine", label: "Vivianne Engine", icon: NeuralNetworkIcon },
  { id: "execution", label: "Execution", icon: PlayCircle02Icon },
  { id: "usage", label: "Usage", icon: Chart01Icon },
  { id: "skills", label: "Skills", icon: MagicWand01Icon },
  { id: "tools", label: "Tools", icon: ToolsIcon },
  { id: "mcp", label: "MCP Servers", icon: McpServerIcon },
  { id: "automations", label: "Automations", icon: AlarmClockIcon },
  { id: "voice", label: "Voice", icon: NeuralNetworkIcon },
];

interface Automation {
  id: string;
  name: string;
  prompt: string;
  cron: string;
  enabled: boolean;
  lastRun: number | null;
  nextRun: number | null;
  createdAt: number;
}

const KOKORO_VOICES = [
  { id: "af_heart", label: "af_heart — female, warm" },
  { id: "af_bella", label: "af_bella — female, expressive" },
  { id: "af_sarah", label: "af_sarah — female, neutral" },
  { id: "am_adam", label: "am_adam — male, neutral" },
  { id: "am_michael", label: "am_michael — male, warm" },
  { id: "bf_emma", label: "bf_emma — female, British" },
  { id: "bm_george", label: "bm_george — male, British" },
];

const BUILT_IN_TOOLS = [
  { id: "read", label: "Read", description: "Read file contents from the workspace." },
  { id: "write", label: "Write", description: "Create or overwrite files." },
  { id: "edit", label: "Edit", description: "Make targeted edits to existing files." },
  { id: "bash", label: "Bash", description: "Execute shell commands (requires a POSIX shell on PATH)." },
  { id: "grep", label: "Grep", description: "Search file contents by regex." },
  { id: "find", label: "Find", description: "Find files by name pattern." },
  { id: "code_search", label: "Code Search", description: "Search the codebase by relevance/meaning for loose or conceptual queries." },
  { id: "ls", label: "List", description: "List directory contents." },
  { id: "delegate", label: "Delegate", description: "Hand off a self-contained subtask to a fresh subagent and get back its final answer." },
  { id: "generate_image", label: "Generate Image", description: "Generate an image from a text prompt (requires an OpenAI API key)." },
];

const DEFAULT_ENGINE_SETTINGS = {
  contextStrategy: "recency",
  recallWindow: 40,
  stateCompression: "off",
};

const panelTransition = { duration: 0.15, ease: [0.16, 1, 0.3, 1] as const };

function SectionHeader({ title, description, badge }: { title: string; description: string; badge?: React.ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <h2 className="text-xl font-bold text-ink font-sans tracking-tight mb-1">{title}</h2>
        <p className="text-body text-sm font-sans max-w-lg">{description}</p>
      </div>
      {badge}
    </div>
  );
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="p-5 border border-hairline/60 bg-canvas/30 backdrop-blur-md rounded-2xl">
      <div className="flex items-center justify-between gap-4">
        <div>
          <label className="text-sm font-bold text-ink font-sans block">{label}</label>
          {description && <span className="text-xs text-body font-sans block mt-0.5 max-w-sm">{description}</span>}
        </div>
        <div className="flex-shrink-0">{children}</div>
      </div>
    </div>
  );
}

function InfoNote({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2 px-4 py-3 rounded-xl bg-surface-soft border border-hairline">
      <HugeiconsIcon icon={InformationCircleIcon} size={14} className="text-body flex-shrink-0 mt-0.5" />
      <p className="text-xs text-body font-sans leading-relaxed">{children}</p>
    </div>
  );
}

const CRON_PRESETS = [
  { label: "Every 15 min", cron: "*/15 * * * *" },
  { label: "Hourly", cron: "0 * * * *" },
  { label: "Daily 9am", cron: "0 9 * * *" },
  { label: "Weekdays 9am", cron: "0 9 * * 1-5" },
  { label: "Weekly (Mon 9am)", cron: "0 9 * * 1" },
];

function AutomationsSection() {
  const [items, setItems] = useState<Automation[]>([]);
  const [name, setName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [cron, setCron] = useState("0 9 * * *");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setItems(await invoke<Automation[]>("cmd_automation_list"));
    } catch (e) {
      console.error("Failed to list automations:", e);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const create = async () => {
    setError(null);
    try {
      await invoke("cmd_automation_create", { name, prompt, cron });
      setName("");
      setPrompt("");
      setCron("0 9 * * *");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const toggle = async (id: string, enabled: boolean) => {
    await invoke("cmd_automation_set_enabled", { id, enabled });
    await refresh();
  };
  const remove = async (id: string) => {
    await invoke("cmd_automation_delete", { id });
    await refresh();
  };
  const runNow = async (id: string) => {
    await invoke("cmd_automation_run_now", { id });
    await refresh();
  };

  const fmtTime = (ms: number | null) =>
    ms ? new Date(ms).toLocaleString() : "—";

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Automations"
        description="Run an agent prompt automatically on a schedule. Fires into the active chat when the agent is idle."
      />
      <InfoNote>
        Schedules use standard 5-field cron (<code className="font-mono">min hour day month weekday</code>).
        A fired automation is skipped if the agent is mid-conversation, and fire times are accurate to ~30 seconds.
      </InfoNote>

      {/* Create form */}
      <div className="p-5 border border-hairline rounded-2xl space-y-3">
        <label className="text-sm font-bold text-ink font-sans block">New Automation</label>
        <Input placeholder="Name (e.g. Morning standup summary)" value={name} onChange={(e) => setName(e.target.value)} className="text-sm" />
        <textarea
          placeholder="Prompt to run (e.g. Summarize today's open PRs and post the top 3)"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={2}
          className="w-full rounded-xl border border-hairline bg-canvas px-3 py-2 text-sm font-sans resize-y focus:outline-none focus:border-hairline-strong"
        />
        <div className="flex gap-2 items-center">
          <Input placeholder="Cron (0 9 * * *)" value={cron} onChange={(e) => setCron(e.target.value)} className="text-xs font-mono flex-1" />
        </div>
        <div className="flex flex-wrap gap-1.5">
          {CRON_PRESETS.map((p) => (
            <button
              key={p.cron}
              onClick={() => setCron(p.cron)}
              className={cn(
                "text-[11px] px-2 py-1 rounded-full border transition-colors font-sans",
                cron === p.cron ? "bg-ink text-on-primary border-ink" : "border-hairline text-body hover:bg-surface-soft"
              )}
            >
              {p.label}
            </button>
          ))}
        </div>
        {error && <p className="text-xs text-rose-600 font-sans">{error}</p>}
        <Button onClick={create} size="sm" disabled={!name.trim() || !prompt.trim()}>
          <HugeiconsIcon icon={PlusSignIcon} size={14} />
          Create
        </Button>
      </div>

      {/* List */}
      <div className="space-y-2">
        {items.length === 0 ? (
          <p className="text-xs text-mute font-sans italic px-1">No automations yet.</p>
        ) : (
          items.map((a) => (
            <div key={a.id} className="p-4 border border-hairline rounded-2xl">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-sm font-semibold font-sans text-ink truncate">{a.name}</p>
                  <p className="text-xs text-body font-sans mt-0.5 line-clamp-2">{a.prompt}</p>
                  <p className="text-[11px] text-mute font-mono mt-1">
                    {a.cron} · next: {fmtTime(a.nextRun)}
                    {a.lastRun ? ` · last: ${fmtTime(a.lastRun)}` : ""}
                  </p>
                </div>
                <div className="flex items-center gap-1.5 flex-shrink-0">
                  <Button variant="ghost" size="icon-sm" onClick={() => runNow(a.id)} title="Run now">
                    <HugeiconsIcon icon={PlayIcon} size={14} />
                  </Button>
                  <Switch checked={a.enabled} onCheckedChange={(v) => toggle(a.id, v)} />
                  <Button variant="ghost" size="icon-sm" onClick={() => remove(a.id)} className="text-mute hover:text-red-500" title="Delete">
                    <HugeiconsIcon icon={Delete02Icon} size={14} />
                  </Button>
                </div>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

interface SettingsPageProps {
  onToggleSidebar?: () => void;
}

export function SettingsPage({ onToggleSidebar }: SettingsPageProps) {
  const {
    state,
    setThinkingLevel,
    setSteeringMode,
    setFollowUpMode,
    getSettings,
    saveSettings,
    listSkills,
    agentReset,
    bypassMode,
    setBypassMode,
    memoryStatus,
    refreshMemoryStatus,
    setMemoryEnabled,
    setMemoryExtractionEnabled,
    purgeMemory,
    setCompactionConfig,
  } = useAgent();

  const [activeSection, setActiveSection] = useState("api-keys");
  const [resetDialogOpen, setResetDialogOpen] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [settings, setSettings] = useState<any>({});

  useEffect(() => {
    let active = true;
    getSettings().then((s) => {
      if (!active) return;
      setSettings(s || {});
      setLoaded(true);
    });
    return () => {
      active = false;
    };
  }, [getSettings]);

  useEffect(() => {
    if (activeSection === "memory") refreshMemoryStatus();
  }, [activeSection, refreshMemoryStatus]);

  const patch = useCallback(
    async (partial: Record<string, any>) => {
      setSettings((prev: any) => {
        const merged = { ...prev, ...partial };
        saveSettings(merged);
        return merged;
      });
    },
    [saveSettings]
  );

  // ── Skills directories + live discovery ──
  const [newSkillPath, setNewSkillPath] = useState("");
  const [discoveredSkills, setDiscoveredSkills] = useState<any[]>([]);
  const [skillErrors, setSkillErrors] = useState<string[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);

  const refreshDiscoveredSkills = useCallback(
    async (paths: string[]) => {
      setSkillsLoading(true);
      const res = await listSkills(paths);
      setDiscoveredSkills(res.skills);
      setSkillErrors(res.errors);
      setSkillsLoading(false);
    },
    [listSkills]
  );

  useEffect(() => {
    if (loaded && activeSection === "skills") {
      refreshDiscoveredSkills(settings.skills || []);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, activeSection]);

  const addSkillPath = () => {
    if (!newSkillPath.trim()) return;
    const updated = [...(settings.skills || []), newSkillPath.trim()];
    setNewSkillPath("");
    patch({ skills: updated });
    refreshDiscoveredSkills(updated);
  };

  const removeSkillPath = (idx: number) => {
    const updated = (settings.skills || []).filter((_: string, i: number) => i !== idx);
    patch({ skills: updated });
    refreshDiscoveredSkills(updated);
  };

  // ── Upload a .md file → save it as a skill in ~/.viv/skills, then rescan ──
  const skillFileInputRef = useRef<HTMLInputElement>(null);
  const [uploadingSkill, setUploadingSkill] = useState(false);
  const handleSkillUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = "";
    if (files.length === 0) return;
    setUploadingSkill(true);
    try {
      for (const file of files) {
        const content = await file.text();
        await invoke("cmd_save_skill_file", { name: file.name, content });
      }
      await refreshDiscoveredSkills(settings.skills || []);
    } catch (err) {
      console.error("Failed to import skill:", err);
    } finally {
      setUploadingSkill(false);
    }
  };

  // ── Tools ──
  const disabledTools: string[] = settings.disabledTools || [];
  const toggleTool = (toolId: string, enabled: boolean) => {
    const updated = enabled ? disabledTools.filter((t) => t !== toolId) : [...disabledTools, toolId];
    patch({ disabledTools: updated });
  };

  // ── MCP servers (config-only; no client wired up yet) ──
  const mcpServers: Array<{ id: string; name: string; command: string; args: string; enabled: boolean }> =
    settings.mcpServers || [];
  const [newMcp, setNewMcp] = useState({ name: "", command: "", args: "" });

  const addMcpServer = () => {
    if (!newMcp.name.trim() || !newMcp.command.trim()) return;
    const server = { id: crypto.randomUUID(), name: newMcp.name.trim(), command: newMcp.command.trim(), args: newMcp.args.trim(), enabled: true };
    patch({ mcpServers: [...mcpServers, server] });
    setNewMcp({ name: "", command: "", args: "" });
  };

  const removeMcpServer = (id: string) => {
    patch({ mcpServers: mcpServers.filter((s) => s.id !== id) });
  };

  const toggleMcpServer = (id: string, enabled: boolean) => {
    patch({ mcpServers: mcpServers.map((s) => (s.id === id ? { ...s, enabled } : s)) });
  };

  // ── Engine (experimental, forward-looking) ──
  const engine = { ...DEFAULT_ENGINE_SETTINGS, ...(settings.engine || {}) };
  const patchEngine = (partial: Record<string, any>) => patch({ engine: { ...engine, ...partial } });

  // ── Memory / compaction ──
  const compaction = { enabled: true, reserveTokens: 16384, keepRecentTokens: 20000, ...(settings.compaction || {}) };
  const patchCompaction = (partial: Record<string, any>) => {
    const merged = { ...compaction, ...partial };
    patch({ compaction: merged });
    setCompactionConfig(merged.enabled, merged.reserveTokens, merged.keepRecentTokens);
  };

  // Push the persisted values to the backend once settings finish loading,
  // so a restart picks up whatever the user last configured (not just the
  // in-memory defaults `AgentState` starts with).
  useEffect(() => {
    if (!loaded) return;
    setCompactionConfig(compaction.enabled, compaction.reserveTokens, compaction.keepRecentTokens);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded]);

  const handleConfirmReset = async () => {
    await agentReset();
    setResetDialogOpen(false);
  };

  return (
    <div className="flex flex-col h-full bg-transparent select-none">
      <header className="h-12 flex items-center gap-2.5 px-4 border-b border-hairline/50 flex-shrink-0">
        {onToggleSidebar && (
          <Button variant="ghost" size="icon-sm" onClick={onToggleSidebar} aria-label="Toggle sidebar">
            <HugeiconsIcon icon={SidebarLeftIcon} size={15} />
          </Button>
        )}
        <h1 className="text-[13px] font-semibold font-sans tracking-tight text-ink">Settings</h1>
      </header>

      <div className="flex-1 flex min-h-0">
        {/* Vertical nav */}
        <nav className="w-52 flex-shrink-0 border-r border-hairline/50 py-4 px-3 overflow-y-auto">
          <div className="space-y-0.5">
            {NAV_SECTIONS.map((section) => {
              const isActive = activeSection === section.id;
              return (
                <button
                  key={section.id}
                  onClick={() => setActiveSection(section.id)}
                  className={cn(
                    "w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-[13px] font-medium font-sans transition-colors text-left",
                    isActive ? "bg-ink text-on-primary" : "text-body hover:text-ink hover:bg-surface-soft"
                  )}
                >
                  <HugeiconsIcon icon={section.icon} size={15} className="flex-shrink-0" />
                  <span className="truncate">{section.label}</span>
                </button>
              );
            })}
          </div>
        </nav>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-6">
          <div className="max-w-2xl mx-auto">
            <AnimatePresence mode="wait">
              <motion.div
                key={activeSection}
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={panelTransition}
              >
                {activeSection === "api-keys" && <ApiKeyManager inline />}

                {activeSection === "memory" && (
                  <div className="space-y-6">
                    <SectionHeader
                      title="Memory"
                      description="Configure how the agent persists and manages conversation context."
                    />

                    <SettingRow label="Memory Provider" description="Persistent backend used to store session history.">
                      <Select value={settings.memoryProvider || "sqlite"} onValueChange={(v) => patch({ memoryProvider: v })}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="sqlite">SQLite (Persistent)</SelectItem>
                          <SelectItem value="memory">In-Memory (Transient)</SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingRow>

                    <SettingRow label="Auto-Compaction" description="Summarize older messages once the context window fills up.">
                      <Switch checked={compaction.enabled} onCheckedChange={(v) => patchCompaction({ enabled: v })} />
                    </SettingRow>

                    <SettingRow
                      label="Reserve Tokens"
                      description="Tokens reserved for the agent's prompt + response before compacting."
                    >
                      <Input
                        type="number"
                        value={compaction.reserveTokens}
                        onChange={(e) => patchCompaction({ reserveTokens: Number(e.target.value) })}
                        className="w-32 text-right font-mono [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                      />
                    </SettingRow>

                    <div className="pt-2 border-t border-hairline" />

                    <SectionHeader
                      title="Project Memory"
                      description="Persistent facts and notes the agent remembers across sessions on this project, via the remember/recall tools."
                    />

                    <SettingRow label="Enabled" description="Inject remembered notes into the system prompt each turn.">
                      <Switch checked={!!memoryStatus?.state.enabled} onCheckedChange={(v) => setMemoryEnabled(v)} />
                    </SettingRow>

                    {memoryStatus?.state.enabled && (
                      <>
                        <SettingRow
                          label="Entries"
                          description={`${memoryStatus.entryCount} saved · ~${memoryStatus.estimatedTokens} tokens`}
                        >
                          <Button variant="destructive" size="sm" onClick={() => purgeMemory()}>
                            Purge All
                          </Button>
                        </SettingRow>

                        <SettingRow
                          label="AI memory extraction"
                          description="Periodically distill saved notes into structured facts and a persona summary, using your current chat model. Off by default — costs extra model calls."
                        >
                          <Switch
                            checked={!!memoryStatus?.state.extractionEnabled}
                            onCheckedChange={(v) => setMemoryExtractionEnabled(v)}
                          />
                        </SettingRow>

                        {memoryStatus.preview && (
                          <div className="p-4 border border-hairline rounded-2xl bg-surface-subtle">
                            <p className="text-[10px] font-bold uppercase tracking-wider text-mute mb-2">Preview</p>
                            <pre className="text-xs font-mono text-body whitespace-pre-wrap max-h-48 overflow-y-auto">
                              {memoryStatus.preview}
                            </pre>
                          </div>
                        )}
                      </>
                    )}

                    <div className="pt-2 border-t border-hairline" />

                    <SettingRow
                      label="Keep Recent Tokens"
                      description="Minimum amount of recent conversation kept verbatim (never summarized)."
                    >
                      <Input
                        type="number"
                        value={compaction.keepRecentTokens}
                        onChange={(e) => patchCompaction({ keepRecentTokens: Number(e.target.value) })}
                        className="w-32 text-right font-mono [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                      />
                    </SettingRow>
                  </div>
                )}

                {activeSection === "engine" && (
                  <div className="space-y-6">
                    <SectionHeader
                      title="Vivianne Engine"
                      description="Context-selection tuning for Vivianne's agent architecture."
                      badge={
                        <Badge variant="outline" className="gap-1">
                          <HugeiconsIcon icon={TestTube02Icon} size={11} />
                          Experimental
                        </Badge>
                      }
                    />

                    <InfoNote>
                      These knobs are saved to your settings file for forward compatibility with upcoming Vivianne
                      engine releases. Some may not yet change runtime behavior — check the changelog before relying
                      on a specific effect.
                    </InfoNote>

                    <SettingRow
                      label="Context Selection Strategy"
                      description="How the engine chooses which prior context to keep in the active window."
                    >
                      <Select value={engine.contextStrategy} onValueChange={(v) => patchEngine({ contextStrategy: v })}>
                        <SelectTrigger className="w-44">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="recency">Recency</SelectItem>
                          <SelectItem value="relevance">Relevance</SelectItem>
                          <SelectItem value="hybrid">Hybrid</SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingRow>

                    <SettingRow
                      label="Recall Window"
                      description="Number of recent turns eligible for selective recall."
                    >
                      <Input
                        type="number"
                        value={engine.recallWindow}
                        onChange={(e) => patchEngine({ recallWindow: Number(e.target.value) })}
                        className="w-32 text-right font-mono [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                      />
                    </SettingRow>

                    <SettingRow
                      label="State Compression"
                      description="Aggressiveness of internal state summarization between turns."
                    >
                      <Select value={engine.stateCompression} onValueChange={(v) => patchEngine({ stateCompression: v })}>
                        <SelectTrigger className="w-36">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="off">Off</SelectItem>
                          <SelectItem value="light">Light</SelectItem>
                          <SelectItem value="aggressive">Aggressive</SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingRow>
                  </div>
                )}

                {activeSection === "execution" && (
                  <div className="space-y-6">
                    <SectionHeader title="Agent Execution" description="Configure the agent's steering and execution policies." />

                    <SettingRow label="Steering Mode" description="Controls whether human feedback is required for each turn.">
                      <div className="flex rounded-full border border-hairline p-0.5 bg-surface-soft gap-0.5">
                        {["one-at-a-time", "all"].map((mode) => (
                          <Button
                            key={mode}
                            size="sm"
                            variant={state.steeringMode === mode ? "default" : "ghost"}
                            onClick={() => setSteeringMode(mode)}
                          >
                            {mode === "all" ? "Autonomous" : "HITL (Steer)"}
                          </Button>
                        ))}
                      </div>
                    </SettingRow>

                    <SettingRow label="Follow-up Mode" description="Controls how follow-up queues are processed.">
                      <div className="flex rounded-full border border-hairline p-0.5 bg-surface-soft gap-0.5">
                        {["one-at-a-time", "all"].map((mode) => (
                          <Button
                            key={mode}
                            size="sm"
                            variant={state.followUpMode === mode ? "default" : "ghost"}
                            onClick={() => setFollowUpMode(mode)}
                          >
                            {mode === "all" ? "All Queue" : "One by One"}
                          </Button>
                        ))}
                      </div>
                    </SettingRow>

                    <SettingRow
                      label="Bypass Mode"
                      description="When on, tool calls (bash, edit, write, apply_patch, computer, web fetch/search) run without asking for approval. When off, each is gated behind a confirmation dialog."
                    >
                      <Switch checked={bypassMode} onCheckedChange={(v) => setBypassMode(v)} />
                    </SettingRow>

                    <div className="p-5 border border-hairline rounded-2xl space-y-3">
                      <label className="text-sm font-bold text-ink font-sans block">Thinking Level</label>
                      <p className="text-xs text-body font-sans -mt-1 block">
                        Pick how thoroughly the LLM thinks or reasons before responding.
                      </p>
                      <div className="flex gap-1.5 flex-wrap pt-1">
                        {["off", "minimal", "low", "medium", "high"].map((level) => (
                          <Button
                            key={level}
                            size="sm"
                            variant={state.thinkingLevel === level ? "default" : "outline"}
                            onClick={() => setThinkingLevel(level)}
                            className="capitalize"
                          >
                            {level}
                          </Button>
                        ))}
                      </div>
                    </div>

                    <div className="p-5 border border-hairline rounded-2xl space-y-3">
                      <label className="text-sm font-bold text-ink font-sans block flex items-center gap-1.5">
                        <HugeiconsIcon icon={Alert02Icon} size={14} className="text-red-500" />
                        Danger Zone
                      </label>
                      <div className="flex items-center justify-between p-4 border border-hairline rounded-xl bg-surface-soft">
                        <div>
                          <p className="text-sm font-medium text-ink font-sans">Reset Conversation</p>
                          <p className="text-xs text-mute font-sans">Wipe the current agent session and messages.</p>
                        </div>
                        <Button variant="destructive" size="sm" onClick={() => setResetDialogOpen(true)}>
                          Reset Session
                        </Button>
                      </div>
                    </div>
                  </div>
                )}

                {activeSection === "usage" && (
                  <div className="space-y-6">
                    <SectionHeader title="Usage" description="Real token and cost usage from your provider calls, tracked per day." />
                    <UsageSettings />
                    <div className="pt-2 border-t border-hairline" />
                    <SettingRow label="App updates" description="Vivianne checks automatically on launch; you can also check now.">
                      <UpdateChecker />
                    </SettingRow>
                  </div>
                )}

                {activeSection === "skills" && (
                  <div className="space-y-6">
                    <SectionHeader title="Skills" description="Directories the agent scans for reusable skill definitions." />

                    {/* Import a skill from a .md file */}
                    <div className="p-5 border border-hairline rounded-2xl space-y-3">
                      <label className="text-sm font-bold text-ink font-sans block">Import a Skill</label>
                      <span className="text-xs text-body font-sans block -mt-1">
                        Upload a Markdown file — it's saved to <code className="font-mono">~/.viv/skills</code> and picked up right away. YAML frontmatter (<code className="font-mono">name</code>, <code className="font-mono">description</code>) is added automatically if missing.
                      </span>
                      <input
                        ref={skillFileInputRef}
                        type="file"
                        accept=".md,.markdown,text/markdown"
                        multiple
                        onChange={handleSkillUpload}
                        className="hidden"
                      />
                      <Button onClick={() => skillFileInputRef.current?.click()} disabled={uploadingSkill} size="sm" className="mt-1">
                        <HugeiconsIcon icon={Upload01Icon} size={14} />
                        {uploadingSkill ? "Importing…" : "Upload .md file"}
                      </Button>
                    </div>

                    <div className="p-5 border border-hairline rounded-2xl space-y-3">
                      <label className="text-sm font-bold text-ink font-sans block">Skill Directories</label>
                      <span className="text-xs text-body font-sans block -mt-1">
                        Paths where the agent searches for SKILL.md files, in addition to <code className="font-mono">~/.viv/skills</code>.
                      </span>

                      <div className="flex gap-2 mt-2">
                        <Input
                          placeholder="e.g. C:\Users\name\skills"
                          value={newSkillPath}
                          onChange={(e) => setNewSkillPath(e.target.value)}
                          className="flex-1 font-mono text-xs"
                        />
                        <Button onClick={addSkillPath} size="icon">
                          <HugeiconsIcon icon={PlusSignIcon} size={16} />
                        </Button>
                      </div>

                      <div className="space-y-1.5 pt-2">
                        {(settings.skills || []).length === 0 ? (
                          <p className="text-xs text-mute font-sans italic">No custom directories added.</p>
                        ) : (
                          (settings.skills || []).map((path: string, idx: number) => (
                            <div key={idx} className="flex items-center justify-between p-2 border border-hairline rounded-xl bg-surface-soft">
                              <span className="text-xs font-mono text-body truncate">{path}</span>
                              <Button variant="ghost" size="icon-sm" onClick={() => removeSkillPath(idx)} className="text-mute hover:text-red-500">
                                <HugeiconsIcon icon={Delete02Icon} size={13} />
                              </Button>
                            </div>
                          ))
                        )}
                      </div>
                    </div>

                    <div className="p-5 border border-hairline rounded-2xl space-y-3">
                      <div className="flex items-center justify-between">
                        <label className="text-sm font-bold text-ink font-sans block">Discovered Skills</label>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => refreshDiscoveredSkills(settings.skills || [])}
                          disabled={skillsLoading}
                        >
                          {skillsLoading ? "Scanning..." : "Rescan"}
                        </Button>
                      </div>
                      {discoveredSkills.length === 0 && skillErrors.length === 0 ? (
                        <p className="text-xs text-mute font-sans italic">No skills found yet.</p>
                      ) : (
                        <div className="space-y-1.5">
                          {discoveredSkills.map((skill: any, idx: number) => (
                            <div key={idx} className="p-3 border border-hairline rounded-xl bg-surface-soft">
                              <div className="flex items-center justify-between gap-2">
                                <p className="text-sm font-semibold font-sans text-ink truncate">{skill.name}</p>
                                <div className="flex items-center gap-1.5 flex-shrink-0">
                                  {skill.status === "missing_dependency" && (
                                    <Badge variant="outline" className="text-amber-600 border-amber-300">
                                      Missing {skill.requires_provider} key
                                    </Badge>
                                  )}
                                  {skill.disable_model_invocation && (
                                    <Badge variant="outline">Manual only</Badge>
                                  )}
                                </div>
                              </div>
                              <p className="text-xs text-body font-sans mt-0.5">{skill.description}</p>
                              <p className="text-[10px] text-mute font-mono mt-1 truncate">{skill.file_path}</p>
                            </div>
                          ))}
                          {skillErrors.map((err, idx) => (
                            <div key={`err-${idx}`} className="p-3 border border-rose-200 rounded-xl bg-rose-50">
                              <p className="text-xs text-rose-700 font-sans">{err}</p>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>
                )}

                {activeSection === "tools" && (
                  <div className="space-y-6">
                    <SectionHeader title="Tools" description="Enable or disable the built-in tools available to the agent." />
                    <div className="space-y-2">
                      {BUILT_IN_TOOLS.map((tool) => {
                        const enabled = !disabledTools.includes(tool.id);
                        return (
                          <div key={tool.id} className="p-4 border border-hairline rounded-2xl flex items-center justify-between gap-4">
                            <div>
                              <p className="text-sm font-semibold font-sans text-ink font-mono">{tool.id}</p>
                              <p className="text-xs text-body font-sans mt-0.5">{tool.description}</p>
                            </div>
                            <Switch checked={enabled} onCheckedChange={(v) => toggleTool(tool.id, v)} />
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}

                {activeSection === "mcp" && (
                  <div className="space-y-6">
                    <SectionHeader
                      title="MCP Servers"
                      description="Register Model Context Protocol servers for the agent to use."
                      badge={
                        <Badge variant="outline" className="gap-1">
                          <HugeiconsIcon icon={TestTube02Icon} size={11} />
                          Preview
                        </Badge>
                      }
                    />

                    <InfoNote>
                      MCP server configuration is saved here for when Vivianne's MCP client ships. Servers added below
                      are not yet connected or queried by the agent.
                    </InfoNote>

                    <div className="p-5 border border-hairline rounded-2xl space-y-3">
                      <label className="text-sm font-bold text-ink font-sans block">Add Server</label>
                      <div className="grid grid-cols-2 gap-2">
                        <Input
                          placeholder="Name (e.g. filesystem)"
                          value={newMcp.name}
                          onChange={(e) => setNewMcp((p) => ({ ...p, name: e.target.value }))}
                          className="text-xs"
                        />
                        <Input
                          placeholder="Command (e.g. npx)"
                          value={newMcp.command}
                          onChange={(e) => setNewMcp((p) => ({ ...p, command: e.target.value }))}
                          className="text-xs font-mono"
                        />
                      </div>
                      <Input
                        placeholder="Args (space-separated)"
                        value={newMcp.args}
                        onChange={(e) => setNewMcp((p) => ({ ...p, args: e.target.value }))}
                        className="text-xs font-mono"
                      />
                      <Button onClick={addMcpServer} size="sm" className="w-full">
                        <HugeiconsIcon icon={PlusSignIcon} size={14} />
                        Add Server
                      </Button>
                    </div>

                    <div className="space-y-2">
                      {mcpServers.length === 0 ? (
                        <p className="text-xs text-mute font-sans italic px-1">No MCP servers configured.</p>
                      ) : (
                        mcpServers.map((server) => (
                          <div key={server.id} className="p-4 border border-hairline rounded-2xl flex items-center justify-between gap-4">
                            <div className="min-w-0">
                              <p className="text-sm font-semibold font-sans text-ink truncate">{server.name}</p>
                              <p className="text-xs text-body font-mono mt-0.5 truncate">
                                {server.command} {server.args}
                              </p>
                            </div>
                            <div className="flex items-center gap-2 flex-shrink-0">
                              <Switch checked={server.enabled} onCheckedChange={(v) => toggleMcpServer(server.id, v)} />
                              <Button
                                variant="ghost"
                                size="icon-sm"
                                onClick={() => removeMcpServer(server.id)}
                                className="text-mute hover:text-red-500"
                              >
                                <HugeiconsIcon icon={Cancel01Icon} size={13} />
                              </Button>
                            </div>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                )}

                {activeSection === "automations" && <AutomationsSection />}

                {activeSection === "voice" && (
                  <div className="space-y-6">
                    <SectionHeader
                      title="Voice Mode"
                      description='Always-on wake-word listener. Say "Vivianne" or "Viv" to start a voice thread.'
                    />

                    <InfoNote>
                      STT runs natively via whisper.cpp in the Rust backend (the GGML model downloads once on first
                      use and is cached). TTS uses Kokoro-82M (ONNX Runtime Web) — its model downloads once on first
                      speech and is cached by the browser.
                    </InfoNote>

                    <SettingRow label="Enabled" description="Listen for the wake word in the background whenever the app is open.">
                      <Switch
                        checked={!!settings.voice?.enabled}
                        onCheckedChange={(v) => patch({ voice: { ...(settings.voice || {}), enabled: v } })}
                      />
                    </SettingRow>

                    <SettingRow label="TTS Engine" description="Kokoro (recommended, offline after first load) or browser SpeechSynthesis.">
                      <Select
                        value={settings.voice?.ttsEngine || "kokoro"}
                        onValueChange={(v) => patch({ voice: { ...(settings.voice || {}), ttsEngine: v } })}
                      >
                        <SelectTrigger className="w-44">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="kokoro">Kokoro (82M ONNX)</SelectItem>
                          <SelectItem value="browser">Browser SpeechSynthesis</SelectItem>
                          <SelectItem value="off">Off (no reply audio)</SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingRow>

                    <SettingRow label="Kokoro Voice" description="Voice preset used when TTS engine is Kokoro.">
                      <Select
                        value={settings.voice?.ttsVoice || "af_heart"}
                        onValueChange={(v) => patch({ voice: { ...(settings.voice || {}), ttsVoice: v } })}
                      >
                        <SelectTrigger className="w-56">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {KOKORO_VOICES.map((v) => (
                            <SelectItem key={v.id} value={v.id}>
                              {v.label}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </SettingRow>

                    <SettingRow
                      label="STT Engine"
                      description="Speech-to-text runs on native whisper.cpp in the Rust backend."
                    >
                      <Select value="whisper" onValueChange={() => {}}>
                        <SelectTrigger className="w-44">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="whisper">whisper.cpp (native)</SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingRow>

                    <SettingRow
                      label="Whisper Model"
                      description="Larger = more accurate but slower. Model downloads once on first use and is cached."
                    >
                      <Select
                        value={settings.voice?.whisperModel || "tiny.en"}
                        onValueChange={(v) => patch({ voice: { ...(settings.voice || {}), whisperModel: v } })}
                      >
                        <SelectTrigger className="w-56">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="tiny.en">tiny.en — fastest (~75 MB)</SelectItem>
                          <SelectItem value="base.en">base.en — balanced (~140 MB)</SelectItem>
                          <SelectItem value="small.en">small.en — accurate (~460 MB)</SelectItem>
                          <SelectItem value="tiny">tiny — multilingual (~75 MB)</SelectItem>
                          <SelectItem value="base">base — multilingual (~140 MB)</SelectItem>
                          <SelectItem value="small">small — multilingual (~460 MB)</SelectItem>
                        </SelectContent>
                      </Select>
                    </SettingRow>
                  </div>
                )}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>

      <Dialog open={resetDialogOpen} onOpenChange={setResetDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Reset conversation?</DialogTitle>
            <DialogDescription>
              This wipes the current agent session and all messages from memory. This cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setResetDialogOpen(false)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleConfirmReset}>
              Reset Session
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
