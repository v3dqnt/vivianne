"use client";

import React, { createContext, useContext, useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { AgentMessage as BackendAgentMessage, ToolArgs, AgentImage } from "./types";

export interface AgentMessage {
  id: string;
  role: "user" | "assistant" | "system" | "toolResult" | "toolCall";
  content: string;
  thinking?: string;
  isStreaming?: boolean;
  toolName?: string;
  toolCallId?: string;
  isError?: boolean;
  args?: ToolArgs | null;
  details?: any;
  status?: "running" | "success" | "error" | "pending";
  images?: AgentImage[];
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  displayName: string;
  api: string;
  /** Real value from the provider's API, or undefined if that provider doesn't expose it. */
  contextWindow?: number;
  maxOutputTokens?: number;
  inputCostPerMillion?: number;
  outputCostPerMillion?: number;
  /** Best-effort capability labels (pattern-matched from the model id, not authoritative). */
  isImageModel?: boolean;
  isAudioModel?: boolean;
  supportsVision?: boolean;
}

interface DiscoveredModel {
  id: string;
  name: string;
  provider: string;
  api: string;
  contextWindow: number | null;
  maxOutputTokens: number | null;
  inputCostPerMillion: number | null;
  outputCostPerMillion: number | null;
  isImageModel: boolean;
  isAudioModel: boolean;
  supportsVision: boolean;
}

export interface ExtensionUIRequest {
  id: string;
  method: "confirm" | "select" | "input" | "editor";
  title: string;
  message?: string;
  options?: string[];
  placeholder?: string;
  prefill?: string;
}

export interface PermissionRequest {
  requestId: string;
  toolCallId: string;
  toolName: string;
  args: any;
  /** Why approval is being asked, e.g. an out-of-workspace path access.
   * Absent for the generic "this tool is always gated" case. */
  reason?: string | null;
}

export interface QuestionOption {
  label: string;
  description: string;
}

export interface QuestionSpec {
  question: string;
  header: string;
  options: QuestionOption[];
  multiSelect?: boolean;
}

export interface QuestionRequest {
  requestId: string;
  toolCallId: string;
  questions: QuestionSpec[];
}

export interface MemoryState {
  enabled: boolean;
  autoConsolidate: boolean;
  createdAt: number;
  updatedAt: number;
  lastInjectedAt: number | null;
  extractionEnabled: boolean;
  lastExtractionAt: number | null;
  lastPersonaSynthesisAt: number | null;
}

export interface UsageDay {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cost: number;
  requests: number;
}

export interface MemoryEntry {
  id: string;
  key: string;
  text: string;
  kind: string;
  sessionId: string | null;
  createdAt: number;
}

export interface MemoryStatus {
  root: string;
  state: MemoryState;
  entryCount: number;
  estimatedTokens: number;
  bytes: number;
  preview: string;
}

export interface SessionStats {
  contextTokens: number;
  contextWindow: number;
  contextPercent: number;
  messageCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheRead: number;
  cacheWrite: number;
  totalTokens: number;
  modelId: string;
  modelName: string;
  provider: string;
  maxTokens: number;
}

export interface SessionSummary {
  id: string;
  name: string;
  created_at: string;
}

export interface AgentState {
  model: ModelInfo | null;
  thinkingLevel: string;
  personaMode: string;
  autoModelEnabled: boolean;
  sessionName: string | null;
  steeringMode: string;
  followUpMode: string;
  autoCompactionEnabled: boolean;
  autoRetryEnabled: boolean;
  messageCount: number;
  pendingMessageCount: number;
}

interface AgentContextType {
  messages: AgentMessage[];
  state: AgentState;
  isReady: boolean;
  isSettingsOpen: boolean;
  setSettingsOpen: (isOpen: boolean) => void;
  isApiKeysOpen: boolean;
  setApiKeysOpen: (isOpen: boolean) => void;
  availableModels: ModelInfo[];
  modelsLoading: boolean;
  refreshAvailableModels: () => Promise<void>;
  tree: any[];
  leafId: string | null;
  sendMessage: (text: string, images?: { base64: string; mediaType: string }[]) => void;
  abortAgent: () => Promise<void>;
  projectDir: string | null;
  pickProjectDir: () => Promise<void>;
  isBusy: boolean;
  loadMessages: () => Promise<void>;
  setModel: (provider: string, modelId: string) => void;
  setAutoModelEnabled: (enabled: boolean) => void;
  setThinkingLevel: (level: string) => void;
  setPersonaMode: (mode: string) => void;
  setSteeringMode: (mode: string) => void;
  setFollowUpMode: (mode: string) => void;
  storeApiKey: (providerId: string, apiKey: string) => Promise<void>;
  deleteApiKey: (providerId: string) => Promise<void>;
  checkAuth: (providerId: string) => Promise<boolean>;
  authenticatedProviders: Set<string>;
  refreshAuthStatus: () => Promise<void>;
  agentStart: () => void;
  agentReset: () => Promise<void>;
  sendCommand: (cmd: any) => void;
  getSettings: () => Promise<any>;
  getUsageHistory: () => Promise<Record<string, UsageDay>>;
  saveSettings: (settings: any) => Promise<void>;
  listSkills: (paths: string[]) => Promise<{ skills: any[]; errors: string[] }>;
  sessions: SessionSummary[];
  refreshSessions: () => Promise<void>;
  loadSession: (id: string) => Promise<void>;
  forkSession: (id: string) => Promise<string>;
  cloneSession: (id: string) => Promise<string>;
  renameSession: (id: string, name: string) => Promise<boolean>;
  sessionStats: SessionStats | null;
  refreshSessionStats: () => Promise<void>;
  uiRequests: ExtensionUIRequest[];
  sendUiResponse: (id: string, response: any) => Promise<void>;
  dismissUiRequest: (id: string) => Promise<void>;
  // Bypass mode / tool-call approval
  bypassMode: boolean;
  setBypassMode: (enabled: boolean) => Promise<void>;
  agentMode: "auto" | "permission";
  setAgentMode: (mode: "auto" | "permission") => Promise<void>;
  permissionRequests: PermissionRequest[];
  respondToPermission: (requestId: string, approved: boolean) => Promise<void>;
  // question tool
  questionRequests: QuestionRequest[];
  answerQuestion: (requestId: string, answers: string[][]) => Promise<void>;
  // Project memory
  memoryStatus: MemoryStatus | null;
  refreshMemoryStatus: () => Promise<void>;
  setMemoryEnabled: (enabled: boolean) => Promise<void>;
  setMemoryExtractionEnabled: (enabled: boolean) => Promise<void>;
  purgeMemory: () => Promise<void>;
  setCompactionConfig: (enabled: boolean, reserveTokens: number, keepRecentTokens: number) => Promise<void>;
  workspaceFolders: string[];
  workspaceAccessMode: "full" | "readOnly";
  refreshWorkspace: () => Promise<void>;
  setAgentWorkspace: (folders: string[], accessMode: "full" | "readOnly") => Promise<void>;
  createWorkspaceFolder: (parent: string, name: string) => Promise<string | null>;
  rememberFact: (text: string) => Promise<void>;
  forgetFact: (query: string) => Promise<number>;
  recallFacts: (query: string) => Promise<MemoryEntry[]>;
}

const AgentContext = createContext<AgentContextType | null>(null);
export const useAgent = () => {
  const ctx = useContext(AgentContext);
  if (!ctx) throw new Error("useAgent must be used within AgentProvider");
  return ctx;
};

export const AgentProvider = ({ children }: { children: React.ReactNode }) => {
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const [state, setState] = useState<AgentState>({
    model: null,
    thinkingLevel: "off",
    personaMode: "agent",
    autoModelEnabled: false,
    sessionName: null,
    steeringMode: "one-at-a-time",
    followUpMode: "one-at-a-time",
    autoCompactionEnabled: true,
    autoRetryEnabled: true,
    messageCount: 0,
    pendingMessageCount: 0,
  });
  const [isReady, setIsReady] = useState(false);
  const [isSettingsOpen, setSettingsOpen] = useState(false);
  const [isApiKeysOpen, setApiKeysOpen] = useState(false);
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [authenticatedProviders, setAuthenticatedProviders] = useState<Set<string>>(new Set());
  const [tree, setTree] = useState<any[]>([]);
  const [leafId, setLeafId] = useState<string | null>(null);
  const [uiRequests, setUiRequests] = useState<ExtensionUIRequest[]>([]);
  const [bypassMode, setBypassModeState] = useState(false);
  const [permissionRequests, setPermissionRequests] = useState<PermissionRequest[]>([]);
  const [questionRequests, setQuestionRequests] = useState<QuestionRequest[]>([]);
  const [memoryStatus, setMemoryStatus] = useState<MemoryStatus | null>(null);

  const refreshMemoryStatus = useCallback(async () => {
    try {
      const status = await invoke<MemoryStatus>("cmd_memory_status");
      setMemoryStatus(status);
    } catch (err) {
      console.error("Failed to load memory status:", err);
    }
  }, []);

  const setMemoryEnabled = useCallback(async (enabled: boolean) => {
    try {
      await invoke(enabled ? "cmd_memory_enable" : "cmd_memory_disable");
      await refreshMemoryStatus();
    } catch (err) {
      console.error("Failed to toggle memory:", err);
    }
  }, [refreshMemoryStatus]);

  const setMemoryExtractionEnabled = useCallback(async (enabled: boolean) => {
    try {
      await invoke("cmd_memory_configure_extraction", { enabled });
      await refreshMemoryStatus();
    } catch (err) {
      console.error("Failed to toggle memory extraction:", err);
    }
  }, [refreshMemoryStatus]);

  const purgeMemory = useCallback(async () => {
    try {
      await invoke("cmd_memory_purge");
      await refreshMemoryStatus();
    } catch (err) {
      console.error("Failed to purge memory:", err);
    }
  }, [refreshMemoryStatus]);

  const setCompactionConfig = useCallback(async (enabled: boolean, reserveTokens: number, keepRecentTokens: number) => {
    try {
      await invoke("cmd_agent_set_compaction_config", { enabled, reserveTokens, keepRecentTokens });
    } catch (err) {
      console.error("Failed to update compaction config:", err);
    }
  }, []);

  const rememberFact = useCallback(async (text: string) => {
    try {
      await invoke("cmd_memory_remember", { text, key: null });
      await refreshMemoryStatus();
    } catch (err) {
      console.error("Failed to save memory:", err);
    }
  }, [refreshMemoryStatus]);

  const forgetFact = useCallback(async (query: string): Promise<number> => {
    try {
      const removed = await invoke<number>("cmd_memory_forget", { query });
      await refreshMemoryStatus();
      return removed;
    } catch (err) {
      console.error("Failed to forget memory:", err);
      return 0;
    }
  }, [refreshMemoryStatus]);

  const recallFacts = useCallback(async (query: string): Promise<MemoryEntry[]> => {
    try {
      return await invoke<MemoryEntry[]>("cmd_memory_recall", { query });
    } catch (err) {
      console.error("Failed to recall memory:", err);
      return [];
    }
  }, []);

  const setBypassMode = useCallback(async (enabled: boolean) => {
    setBypassModeState(enabled);
    try {
      const current = await invoke<any>("cmd_get_settings");
      await invoke("cmd_save_settings", { settings: { ...current, bypassMode: enabled } });
      // Draining already-pending approvals when bypass turns on happens
      // backend-side the next time a gated tool call checks the flag; any
      // requests already waiting get cleaned up as PermissionResolved arrives.
    } catch (err) {
      console.error("Failed to save bypass mode:", err);
    }
  }, []);

  const agentMode: "auto" | "permission" = bypassMode ? "auto" : "permission";
  const setAgentMode = useCallback(async (mode: "auto" | "permission") => {
    setBypassModeState(mode === "auto");
    try {
      const current = await invoke<any>("cmd_get_settings");
      await invoke("cmd_save_settings", { settings: { ...current, bypassMode: mode === "auto" } });
    } catch (err) {
      console.error("Failed to save agent mode:", err);
    }
  }, []);

  const respondToPermission = useCallback(async (requestId: string, approved: boolean) => {
    setPermissionRequests(prev => prev.filter(r => r.requestId !== requestId));
    try {
      await invoke("cmd_agent_respond_permission", { requestId, approved });
    } catch (err) {
      console.error("Failed to respond to permission request:", err);
    }
  }, []);

  const answerQuestion = useCallback(async (requestId: string, answers: string[][]) => {
    setQuestionRequests(prev => prev.filter(r => r.requestId !== requestId));
    try {
      await invoke("cmd_agent_answer_question", { requestId, answers });
    } catch (err) {
      console.error("Failed to answer question:", err);
    }
  }, []);

  const sendUiResponse = useCallback(async (id: string, response: any) => {
    setUiRequests(prev => prev.filter(r => r.id !== id));
  }, []);

  const dismissUiRequest = useCallback(async (id: string) => {
    setUiRequests(prev => prev.filter(r => r.id !== id));
  }, []);

  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [sessionStats, setSessionStats] = useState<SessionStats | null>(null);
  const [workspaceFolders, setWorkspaceFolders] = useState<string[]>([]);
  const [workspaceAccessMode, setWorkspaceAccessMode] = useState<"full" | "readOnly">("full");

  // The frontend's only source of truth for what a session load restored
  // server-side (`cmd_load_session` sets `AgentState` directly, no event for it).
  const refreshWorkspace = useCallback(async () => {
    try {
      const res = await invoke<{ folders: string[]; accessMode: string }>("cmd_agent_get_workspace");
      setWorkspaceFolders(res.folders ?? []);
      setWorkspaceAccessMode(res.accessMode === "readOnly" ? "readOnly" : "full");
    } catch (err) {
      console.error("Failed to load workspace:", err);
    }
  }, []);

  const refreshSessionStats = useCallback(async () => {
    try {
      const stats = await invoke<SessionStats>("cmd_agent_session_stats");
      setSessionStats(stats);
    } catch (err) {
      console.error("Failed to load session stats:", err);
    }
  }, []);

  // Load messages from backend — declared first so session hooks can reference it
  const loadMessages = useCallback(async () => {
    try {
      const msgs = await invoke<BackendAgentMessage[]>("cmd_agent_messages");
      const mapped = msgs.map((m: any) => mapBackendMessage(m));
      // Guard against a backend serialization that yields an empty assistant
      // turn: don't let a blank reload clobber text we already streamed for the
      // same message. Match by id, else by position among assistant messages.
      setMessages((prev) => {
        const prevAssistants = prev.filter((p) => p.role === "assistant" && p.content);
        let aIdx = 0;
        return mapped.map((m) => {
          if (m.role !== "assistant") return m;
          const streamed = m.content
            ? null
            : (m.id && prev.find((p) => p.id === m.id && p.content)) || prevAssistants[aIdx];
          aIdx++;
          if (!m.content && streamed?.content) {
            return { ...m, content: streamed.content };
          }
          return m;
        });
      });
    } catch (err) {
      console.error("Failed to load messages:", err);
    }
  }, []);

  const refreshSessions = useCallback(async () => {
    try {
      const list = await invoke<SessionSummary[]>("cmd_get_sessions");
      setSessions(list);
    } catch (err) {
      console.error("Failed to load sessions:", err);
    }
  }, []);

  const loadSession = useCallback(async (id: string) => {
    try {
      await invoke("cmd_load_session", { id });
      await loadMessages();
      await refreshSessionStats();
      await refreshWorkspace();
      setState((s) => ({ ...s, sessionName: sessions.find((sess) => sess.id === id)?.name ?? s.sessionName }));
    } catch (err) {
      console.error("Failed to load session:", err);
    }
  }, [loadMessages, refreshSessionStats, refreshWorkspace, sessions]);

  const forkSession = useCallback(async (id: string) => {
    try {
      const newId = await invoke<string>("cmd_fork_session", { id });
      await refreshSessions();
      await loadMessages();
      return newId;
    } catch (err) {
      console.error("Failed to fork session:", err);
      return "";
    }
  }, [refreshSessions, loadMessages]);

  const cloneSession = useCallback(async (id: string) => {
    try {
      const newId = await invoke<string>("cmd_clone_session", { id });
      await refreshSessions();
      await loadMessages();
      return newId;
    } catch (err) {
      console.error("Failed to clone session:", err);
      return "";
    }
  }, [refreshSessions, loadMessages]);

  const renameSession = useCallback(async (id: string, name: string) => {
    try {
      await invoke("cmd_rename_session", { id, name });
      await refreshSessions();
      return true;
    } catch (err) {
      console.error("Failed to rename session:", err);
      return false;
    }
  }, [refreshSessions]);

  const handleBackendEvent = useCallback((payload: any) => {
    const getField = (name: string) => {
      const snake = name.replace(/[A-Z]/g, l => `_${l.toLowerCase()}`);
      if (payload[name] !== undefined) return payload[name];
      if (payload[snake] !== undefined) return payload[snake];
      if (payload.payload) {
        if (payload.payload[name] !== undefined) return payload.payload[name];
        if (payload.payload[snake] !== undefined) return payload.payload[snake];
      }
      return undefined;
    };

    switch (payload.type) {
      case "message_start":
      case "MessageStart": {
        const msg = getField("message");
        const role = msg?.type || msg?.role;
        if (role === "assistant") {
          setMessages(prev => [...prev, {
            id: crypto.randomUUID(),
            role: "assistant",
            content: "",
            isStreaming: true,
          }]);
        }
        break;
      }
      case "message_update":
      case "MessageUpdate": {
        const chunk = getField("chunk");
        setMessages(prev => {
          const last = [...prev];
          const idx = last.findIndex(m => m.isStreaming && m.role === "assistant");
          if (idx >= 0) {
            last[idx] = { ...last[idx], content: last[idx].content + (chunk || "") };
          }
          return last;
        });
        break;
      }
      case "message_end":
      case "MessageEnd": {
        const fullMsg = getField("message");
        let fullText = "";
        let thinkingText = "";
        let isError = false;
        if (fullMsg) {
          const errorMessage = fullMsg.errorMessage ?? fullMsg.error_message;
          isError = !!errorMessage;
          if (typeof fullMsg.content === "string") {
            fullText = fullMsg.content;
          } else if (Array.isArray(fullMsg.content)) {
            for (const c of fullMsg.content) {
              if (typeof c === "string") { fullText += c; }
              else if (c?.text !== undefined) { fullText += c.text; }
              else if (c?.thinking !== undefined) { thinkingText += c.thinking; }
              else if (c?.Thinking?.thinking !== undefined) { thinkingText += c.Thinking.thinking; }
            }
          }
          if (errorMessage) {
            if (fullText) fullText += "\n\nError: " + errorMessage;
            else fullText = "Error: " + errorMessage;
          }
        }
        setMessages(prev => prev.map(m => {
          if (!m.isStreaming || m.role !== "assistant") return m;
          return {
            ...m,
            // Trust the authoritative final message over whatever accumulated
            // from streamed chunks — a provider quirk in the delta stream
            // shouldn't leak into the saved/spoken content.
            content: fullText || m.content,
            thinking: thinkingText || undefined,
            isStreaming: false,
            isError,
          };
        }));
        break;
      }
      case "agent_end":
      case "AgentEnd":
        setIsBusy(false);
        loadMessages();
        refreshSessionStats();
        break;
      case "turn_end":
      case "TurnEnd":
        loadMessages();
        refreshSessionStats();
        break;
      case "permission_asked":
      case "PermissionAsked": {
        const requestId = getField("requestId");
        const toolCallId = getField("toolCallId");
        const toolName = getField("toolName");
        const args = getField("args");
        const reason = getField("reason");
        if (!requestId) break;
        setPermissionRequests(prev =>
          prev.some(r => r.requestId === requestId) ? prev : [...prev, { requestId, toolCallId, toolName, args, reason }]
        );
        break;
      }
      case "permission_resolved":
      case "PermissionResolved": {
        const requestId = getField("requestId");
        setPermissionRequests(prev => prev.filter(r => r.requestId !== requestId));
        break;
      }
      case "question_asked":
      case "QuestionAsked": {
        const requestId = getField("requestId");
        const toolCallId = getField("toolCallId");
        const questions = getField("questions");
        if (!requestId || !Array.isArray(questions)) break;
        setQuestionRequests(prev =>
          prev.some(r => r.requestId === requestId) ? prev : [...prev, { requestId, toolCallId, questions }]
        );
        break;
      }
      case "question_resolved":
      case "QuestionResolved": {
        const requestId = getField("requestId");
        setQuestionRequests(prev => prev.filter(r => r.requestId !== requestId));
        break;
      }
      case "tool_execution_start":
      case "ToolExecutionStart": {
        const toolName = getField("toolName");
        const toolCallId = getField("toolCallId");
        const argsRaw = getField("args");
        const args = parseToolArgs(toolName, argsRaw);
        setMessages(prev => [...prev, {
          id: toolCallId || crypto.randomUUID(),
          role: "toolResult",
          content: "",
          toolName,
          toolCallId,
          args,
          isStreaming: true,
          status: "running",
        }]);
        break;
      }
      case "tool_execution_update":
      case "ToolExecutionUpdate": {
        // Partial result — surface in the running card without ending it
        const toolCallId = getField("toolCallId");
        const partialResult = getField("partialResult");
        if (!toolCallId) break;
        const text = typeof partialResult === "string"
          ? partialResult
          : partialResult && typeof partialResult === "object"
            ? extractContent(partialResult)
            : "";
        setMessages(prev => {
          const idx = prev.findIndex(m => m.toolCallId === toolCallId);
          if (idx < 0) return prev;
          const updated = [...prev];
          const existing = updated[idx];
          updated[idx] = {
            ...existing,
            content: existing.content ? existing.content + text : text,
            isStreaming: true,
            status: existing.status ?? "running",
          };
          return updated;
        });
        break;
      }
      case "tool_execution_end":
      case "ToolExecutionEnd": {
        const toolCallId = getField("toolCallId");
        const toolName = getField("toolName");
        const result = getField("result");
        const isError = !!getField("isError");

        // Try multiple shapes for the result because the backend and forwarded payloads
        // have evolved independently for older sessions vs live runs.
        let resultText = "";
        let details: any = null;
        let resultImages: AgentImage[] = [];
        if (result) {
          if (typeof result === "string") {
            resultText = result;
          } else if (Array.isArray(result.content)) {
            resultText = extractContent(result);
            resultImages = extractImages(result);
          } else if (typeof result.content === "string") {
            resultText = result.content;
          } else if (Array.isArray(result)) {
            resultText = result.map((c: any) => (typeof c === "string" ? c : c?.text ?? "")).join("");
          }
          if (result.details !== undefined) {
            details = result.details;
          } else if (result && typeof result === "object" && !Array.isArray(result.content) && result.content && typeof result.content !== "string") {
            details = result;
          }
        }

        setMessages(prev => {
          const existingIdx = prev.findIndex(m => m.toolCallId === toolCallId);
          if (existingIdx >= 0) {
            const updated = [...prev];
            const existing = updated[existingIdx];
            updated[existingIdx] = {
              ...existing,
              content: resultText || existing.content || getToolFallback(toolName, isError),
              isStreaming: false,
              isError,
              status: isError ? "error" : "success",
              args: existing.args ?? parseToolArgs(toolName, getField("args")),
              details: details ?? existing.details,
              images: resultImages.length ? resultImages : existing.images,
            };
            return updated;
          } else {
            return [...prev, {
              id: toolCallId || crypto.randomUUID(),
              role: "toolResult",
              toolName,
              toolCallId,
              content: resultText || getToolFallback(toolName, isError),
              args: parseToolArgs(toolName, getField("args")),
              details,
              isStreaming: false,
              isError,
              status: isError ? "error" : "success",
              images: resultImages.length ? resultImages : undefined,
            }];
          }
        });
        break;
      }
    }
  }, [loadMessages, refreshSessionStats]);

  const handleBackendEventRef = useRef(handleBackendEvent);
  useEffect(() => {
    handleBackendEventRef.current = handleBackendEvent;
  }, [handleBackendEvent]);

  // Listen for agent events — all streaming deltas come via agent_event MessageUpdate
  useEffect(() => {
    let unlistenAgent: UnlistenFn | null = null;
    const setup = async () => {
      unlistenAgent = await listen("agent_event", (event: any) => {
        const payload = event.payload;
        if (!payload) return;
        handleBackendEventRef.current(payload);
      });
    };
    setup();
    return () => {
      if (unlistenAgent) unlistenAgent();
    };
  }, []);

  // Push a model to the backend without persisting it as "last model" — used
  // by auto mode, which re-picks per turn and shouldn't have that pick stick
  // around as the manually-selected model on next launch.
  const applyModelSilently = useCallback(async (model: ModelInfo) => {
    await invoke("cmd_agent_set_model", {
      id: model.id,
      name: model.name,
      api: model.api,
      provider: model.provider,
      contextWindow: model.contextWindow ?? null,
      maxTokens: model.maxOutputTokens ?? null,
    });
    setState(s => ({ ...s, model }));
  }, []);

  const setAutoModelEnabled = useCallback((enabled: boolean) => {
    setState(s => ({ ...s, autoModelEnabled: enabled }));
  }, []);

  const setModel = useCallback(async (provider: string, modelId: string) => {
    const model = availableModels.find(m => m.provider === provider && m.id === modelId);
    if (!model) return;
    try {
      // Manually picking a model overrides auto mode.
      setState(s => ({ ...s, autoModelEnabled: false }));
      await applyModelSilently(model);
      // Remember the choice so it's restored on next launch. (Auto mode's
      // per-turn picks go through applyModelSilently directly and skip this.)
      try {
        const current = await invoke<any>("cmd_get_settings");
        await invoke("cmd_save_settings", {
          settings: { ...current, lastModel: { provider: model.provider, id: model.id } },
        });
      } catch {
        /* non-fatal */
      }
    } catch (err) {
      console.error("Failed to set model in backend:", err);
    }
  }, [availableModels, applyModelSilently]);

  const sendMessage = useCallback(async (text: string, images?: { base64: string; mediaType: string }[]) => {
    setMessages(prev => [...prev, { id: crypto.randomUUID(), role: "user", content: text, images }]);
    setIsBusy(true);
    try {
      // Auto mode: pick the best available model for this turn before sending.
      if (state.autoModelEnabled && availableModels.length > 0) {
        const { chooseBestModel } = await import("@/lib/modelRouter");
        const picked = chooseBestModel({ models: availableModels, prompt: text, hasImages: !!images?.length });
        if (picked) await applyModelSilently(picked);
      }
      const payloadImages = images?.length
        ? images.map((img) => ({ base64: img.base64, media_type: img.mediaType }))
        : null;
      await invoke("cmd_agent_prompt", { text, images: payloadImages });
      await refreshSessions();
    } catch (err) {
      console.error("Failed to send message:", err);
      setMessages(prev => [...prev, { id: crypto.randomUUID(), role: "assistant", content: "Error: Failed to send message." }]);
      setIsBusy(false);
    }
  }, [refreshSessions, state.autoModelEnabled, availableModels, applyModelSilently]);

  // When a scheduled automation fires, run its prompt through the agent — but
  // only if the agent is idle, so a timer never clobbers an active conversation.
  // ponytail: fires into the current chat; a fired-while-busy automation is
  // skipped (not queued). Upgrade path: run each automation in its own session.
  const busyRef = useRef(false);
  useEffect(() => { busyRef.current = isBusy; }, [isBusy]);
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    (async () => {
      unlisten = await listen("automation_fired", (event: any) => {
        const prompt = event?.payload?.prompt;
        if (!prompt) return;
        if (busyRef.current) {
          console.warn("[automation] fired while agent busy — skipping:", event.payload?.name);
          return;
        }
        sendMessage(`[automation: ${event.payload?.name ?? "scheduled"}] ${prompt}`);
      });
    })();
    return () => { if (unlisten) unlisten(); };
  }, [sendMessage]);

  const [projectDir, setProjectDirState] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>("cmd_get_project_dir").then(setProjectDirState).catch(() => {});
  }, []);

  const pickProjectDir = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({ directory: true, multiple: false, defaultPath: projectDir ?? undefined });
      if (!selected || Array.isArray(selected)) return;
      const confirmed = await invoke<string>("cmd_set_project_dir", { path: selected });
      setProjectDirState(confirmed);
    } catch (err) {
      console.error("Failed to set project directory:", err);
    }
  }, [projectDir]);

  const setAgentWorkspace = useCallback(async (folders: string[], accessMode: "full" | "readOnly") => {
    try {
      await invoke("cmd_agent_set_workspace", { folders, accessMode });
      setWorkspaceFolders(folders);
      setWorkspaceAccessMode(accessMode);
    } catch (err) {
      console.error("Failed to set agent workspace:", err);
    }
  }, []);

  const createWorkspaceFolder = useCallback(async (parent: string, name: string) => {
    try {
      return await invoke<string>("cmd_create_workspace_folder", { parent, name });
    } catch (err) {
      console.error("Failed to create workspace folder:", err);
      return null;
    }
  }, []);

  useEffect(() => {
    refreshWorkspace();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const abortAgent = useCallback(async () => {
    try {
      await invoke("cmd_agent_abort");
    } catch (err) {
      console.error("Failed to abort agent:", err);
    } finally {
      setIsBusy(false);
    }
  }, []);

  const storeApiKey = useCallback(async (providerId: string, apiKey: string) => {
    try {
      await invoke("auth_store_api_key", { providerId, apiKey });
      await refreshAuthStatus();
    } catch (err) {
      console.error("Failed to store API key:", err);
    }
  }, []);

  const deleteApiKey = useCallback(async (providerId: string) => {
    try {
      await invoke("auth_delete_credential", { providerId });
      await refreshAuthStatus();
    } catch (err) {
      console.error("Failed to delete API key:", err);
    }
  }, []);

  const checkAuth = useCallback(async (providerId: string): Promise<boolean> => {
    try {
      return await invoke("auth_is_authenticated", { providerId });
    } catch {
      return false;
    }
  }, []);

  const refreshAuthStatus = useCallback(async () => {
    const providers = [
      "anthropic", "openai", "google", "mistral", "nvidia", "nvidia-nim", "bedrock",
      "groq", "xai", "deepseek", "together", "fireworks", "openrouter",
      "azure-openai-responses", "cerebras", "huggingface", "cloudflare-workers-ai"
    ];
    const authSet = new Set<string>();
    for (const p of providers) {
      if (await checkAuth(p)) authSet.add(p);
    }
    setAuthenticatedProviders(authSet);
  }, [checkAuth]);

  const agentReset = useCallback(async () => {
    await invoke("cmd_agent_reset");
    setMessages([]);
    setState((s) => ({ ...s, sessionName: null }));
    setSessionStats(null);
  }, []);

  const setThinkingLevel = useCallback(async (level: string) => {
    try {
      await invoke("cmd_agent_set_thinking_level", { level });
      setState(s => ({ ...s, thinkingLevel: level }));
    } catch (err) {
      console.error("Failed to set thinking level in backend:", err);
    }
  }, []);

  const setPersonaMode = useCallback(async (mode: string) => {
    try {
      await invoke("cmd_agent_set_persona_mode", { mode });
      setState(s => ({ ...s, personaMode: mode }));
    } catch (err) {
      console.error("Failed to set persona mode in backend:", err);
    }
  }, []);

  const setSteeringMode = useCallback(async (mode: string) => {
    try {
      await invoke("cmd_agent_set_steering_mode", { mode });
      setState(s => ({ ...s, steeringMode: mode }));
    } catch (err) {
      console.error("Failed to set steering mode in backend:", err);
    }
  }, []);

  const setFollowUpMode = useCallback(async (mode: string) => {
    try {
      await invoke("cmd_agent_set_follow_up_mode", { mode });
      setState(s => ({ ...s, followUpMode: mode }));
    } catch (err) {
      console.error("Failed to set follow up mode in backend:", err);
    }
  }, []);

  const sendCommand = useCallback((cmd: any) => {
    // Legacy command handler
    if (cmd.type === "set_model") {
      setModel(cmd.provider, cmd.modelId);
    } else if (cmd.type === "set_thinking_level") {
      setThinkingLevel(cmd.level);
    } else if (cmd.type === "set_steering_mode") {
      setSteeringMode(cmd.mode);
    } else if (cmd.type === "set_follow_up_mode") {
      setFollowUpMode(cmd.mode);
    }
  }, [setModel, setThinkingLevel, setSteeringMode, setFollowUpMode]);

  const getUsageHistory = useCallback(async () => {
    try {
      return await invoke<Record<string, UsageDay>>("cmd_get_usage_history");
    } catch (err) {
      console.error("Failed to get usage history:", err);
      return {};
    }
  }, []);

  const getSettings = useCallback(async () => {
    try {
      return await invoke<any>("cmd_get_settings");
    } catch (err) {
      console.error("Failed to get settings:", err);
      return {};
    }
  }, []);

  const saveSettings = useCallback(async (settings: any) => {
    try {
      await invoke("cmd_save_settings", { settings });
    } catch (err) {
      console.error("Failed to save settings:", err);
    }
  }, []);

  const listSkills = useCallback(async (paths: string[]) => {
    try {
      return await invoke<{ skills: any[]; errors: string[] }>("cmd_list_skills", { paths });
    } catch (err) {
      console.error("Failed to list skills:", err);
      return { skills: [], errors: [String(err)] };
    }
  }, []);

  useEffect(() => {
    setIsReady(true);
    loadMessages();
    refreshAuthStatus();
    refreshSessions();
    refreshSessionStats();
    refreshMemoryStatus();
    invoke<any>("cmd_get_settings")
      .then((settings) => setBypassModeState(!!settings?.bypassMode))
      .catch((err) => console.error("Failed to load bypass mode:", err));
  }, [loadMessages, refreshAuthStatus, refreshSessions, refreshSessionStats, refreshMemoryStatus]);

  const [modelsLoading, setModelsLoading] = useState(false);

  const refreshAvailableModels = useCallback(async () => {
    setModelsLoading(true);
    try {
      const discovered = await invoke<DiscoveredModel[]>("cmd_list_available_models");
      const models: ModelInfo[] = discovered.map((m) => ({
        id: m.id,
        name: m.name,
        provider: m.provider,
        displayName: m.name,
        api: m.api,
        contextWindow: m.contextWindow ?? undefined,
        maxOutputTokens: m.maxOutputTokens ?? undefined,
        inputCostPerMillion: m.inputCostPerMillion ?? undefined,
        outputCostPerMillion: m.outputCostPerMillion ?? undefined,
        isImageModel: m.isImageModel,
        isAudioModel: m.isAudioModel,
        supportsVision: m.supportsVision,
      }));
      setAvailableModels(models);

      // Restore the last-chosen model from settings, falling back to the first.
      let preferred: ModelInfo | null = models[0] ?? null;
      try {
        const cfg = await invoke<any>("cmd_get_settings");
        const last = cfg?.lastModel;
        if (last) {
          const match = models.find((m) => m.id === last.id && m.provider === last.provider);
          if (match) preferred = match;
        }
      } catch {
        /* fall back to models[0] */
      }

      setState((s) => {
        if (models.length > 0) {
          const isCurrentValid = s.model && models.some((m) => m.id === s.model?.id && m.provider === s.model?.provider);
          if (!isCurrentValid && preferred) {
            const newModel = preferred;
            invoke("cmd_agent_set_model", {
              id: newModel.id,
              name: newModel.name,
              api: newModel.api,
              provider: newModel.provider,
              contextWindow: newModel.contextWindow,
              maxTokens: newModel.maxOutputTokens,
            }).catch((e) => console.error("[AgentProvider] Failed to sync model to backend:", e));
            return { ...s, model: newModel };
          }
        } else {
          return { ...s, model: null };
        }
        return s;
      });
    } catch (err) {
      console.error("Failed to discover available models:", err);
    } finally {
      setModelsLoading(false);
    }
  }, []);

  useEffect(() => {
    refreshAvailableModels();
  }, [authenticatedProviders, refreshAvailableModels]);

  return (
    <AgentContext.Provider value={{
      messages,
      state,
      isReady,
      isSettingsOpen,
      setSettingsOpen,
      isApiKeysOpen,
      setApiKeysOpen,
      availableModels,
      modelsLoading,
      refreshAvailableModels,
      tree,
      leafId,
      sendMessage,
      abortAgent,
      projectDir,
      pickProjectDir,
      isBusy,
      loadMessages,
      setModel,
      setAutoModelEnabled,
      setThinkingLevel,
      setPersonaMode,
      setSteeringMode,
      setFollowUpMode,
      storeApiKey,
      deleteApiKey,
      checkAuth,
      authenticatedProviders,
      refreshAuthStatus,
      agentStart: () => {},
      agentReset,
      sendCommand,
      getSettings,
      saveSettings,
      listSkills,
      sessions,
      refreshSessions,
      loadSession,
      forkSession,
      cloneSession,
      renameSession,
      sessionStats,
      refreshSessionStats,
      uiRequests,
      sendUiResponse,
      dismissUiRequest,
      bypassMode,
      setBypassMode,
      agentMode,
      setAgentMode,
      permissionRequests,
      respondToPermission,
      questionRequests,
      answerQuestion,
      memoryStatus,
      refreshMemoryStatus,
      setMemoryEnabled,
      setMemoryExtractionEnabled,
      purgeMemory,
      setCompactionConfig,
      workspaceFolders,
      workspaceAccessMode,
      refreshWorkspace,
      setAgentWorkspace,
      createWorkspaceFolder,
      getUsageHistory,
      rememberFact,
      forgetFact,
      recallFacts,
    }}>
      {children}
    </AgentContext.Provider>
  );
};

function extractContent(m: any): string {
  let content = "";
  if (m.content) {
    if (typeof m.content === "string") {
      content = m.content;
    } else if (Array.isArray(m.content)) {
      content = m.content.map((c: any) => {
        if (typeof c === "string") return c;
        if (c?.text !== undefined) return c.text;
        // Thinking blocks are extracted separately — skip here
        if (c?.thinking !== undefined || c?.Thinking !== undefined) return "";
        return JSON.stringify(c);
      }).filter(Boolean).join("");
    } else {
      content = JSON.stringify(m.content);
    }
  }
  const errorMessage = m.errorMessage ?? m.error_message;
  if (errorMessage) {
    if (content) content += "\n\nError: " + errorMessage;
    else content = "Error: " + errorMessage;
  }
  return content;
}

function extractThinking(m: any): string {
  if (!Array.isArray(m?.content)) return "";
  return m.content
    .map((c: any) => c?.thinking ?? c?.Thinking?.thinking ?? "")
    .filter(Boolean)
    .join("\n\n");
}

function extractImages(m: any): AgentImage[] {
  if (!Array.isArray(m?.content)) return [];
  return m.content
    .map((c: any) => {
      const img = c?.type === "image" ? c : c?.Image;
      if (!img) return null;
      const base64 = img.base64;
      if (!base64) return null;
      return { base64, mediaType: img.mediaType ?? img.media_type ?? "image/png" };
    })
    .filter((img: AgentImage | null): img is AgentImage => img !== null);
}

function mapBackendMessage(m: any): AgentMessage {
  const role: AgentMessage["role"] =
    m.role === "user" || m.role === "User" || m.type === "user" || m.type === "User" ? "user"
    : m.role === "assistant" || m.role === "Assistant" || m.type === "assistant" || m.type === "Assistant" ? "assistant"
    : m.role === "toolResult" || m.role === "ToolResult" || m.type === "toolResult" || m.type === "ToolResult" ? "toolResult"
    : "system";

  const isTool = role === "toolResult";
  let args: ToolArgs | null = null;
  let details: any = null;

  const toolName = m.toolName ?? m.tool_name;
  const toolCallId = m.toolCallId ?? m.tool_call_id;
  const isError = m.isError ?? m.is_error;

  if (isTool) {
    args = parseToolArgs(toolName, m.arguments ?? m.args);
    details = m.details ?? null;
  }

  const images = extractImages(m);

  return {
    id: m.id || Math.random().toString(36).substring(7),
    role,
    content: extractContent(m),
    thinking: role === "assistant" ? extractThinking(m) || undefined : undefined,
    isStreaming: false,
    toolName,
    toolCallId,
    isError: !!isError,
    args,
    details,
    status: isTool ? (isError ? "error" : "success") : undefined,
    images: images.length ? images : undefined,
  };
}

function parseToolArgs(toolName: string | undefined, raw: any): ToolArgs | null {
  if (!toolName || raw == null) return null;
  let obj: any = raw;
  if (typeof obj === "string") {
    try { obj = JSON.parse(obj); } catch { return null; }
  }
  if (!obj || typeof obj !== "object") return null;
  const pick = (k: string) => {
    const v = obj[k] ?? obj[k.toLowerCase()] ?? obj[k.replace(/_([a-z])/g, (_, c) => c.toUpperCase())];
    return v === undefined || v === null ? undefined : v;
  };
  // Spread the raw object first so tool-specific keys (url, query, action,
  // seconds, todos, notebook_path, edit_mode, coordinate, text…) survive for
  // the newer tools, then overlay the normalized common keys.
  return {
    ...obj,
    path: pick("path"),
    command: pick("command"),
    pattern: pick("pattern"),
    offset: pick("offset"),
    limit: pick("limit"),
    content: pick("content"),
    old_string: pick("old_string"),
    new_string: pick("new_string"),
  };
}

function getToolFallback(toolName: string | undefined, isError: boolean): string {
  if (isError) return `${toolName ?? "Tool"} failed`;
  return `${toolName ?? "Tool"} completed`;
}
