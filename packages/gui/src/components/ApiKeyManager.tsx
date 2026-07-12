"use client";

import React, { useState } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { Key01Icon, ViewIcon, ViewOffSlashIcon, Delete02Icon, Tick02Icon, Alert02Icon } from "@hugeicons/core-free-icons";
import { useAgent } from "./AgentProvider";
import { cn } from "@/lib/utils";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ProviderIcon } from "@/components/ProviderIcons";

interface ProviderConfig {
  id: string;
  name: string;
  envVar: string;
}

const PROVIDERS: ProviderConfig[] = [
  { id: "anthropic", name: "Anthropic (Claude)", envVar: "ANTHROPIC_API_KEY" },
  { id: "openai", name: "OpenAI", envVar: "OPENAI_API_KEY" },
  { id: "google", name: "Google AI", envVar: "GEMINI_API_KEY" },
  { id: "nvidia", name: "NVIDIA", envVar: "NVIDIA_API_KEY" },
  { id: "nvidia-nim", name: "NVIDIA NIM", envVar: "NVIDIA_NIM_API_KEY" },
  { id: "groq", name: "Groq", envVar: "GROQ_API_KEY" },
  { id: "mistral", name: "Mistral AI", envVar: "MISTRAL_API_KEY" },
  { id: "xai", name: "xAI (Grok)", envVar: "XAI_API_KEY" },
  { id: "deepseek", name: "DeepSeek", envVar: "DEEPSEEK_API_KEY" },
  { id: "together", name: "Together AI", envVar: "TOGETHER_API_KEY" },
  { id: "fireworks", name: "Fireworks", envVar: "FIREWORKS_API_KEY" },
  { id: "openrouter", name: "OpenRouter", envVar: "OPENROUTER_API_KEY" },
  { id: "azure-openai-responses", name: "Azure OpenAI", envVar: "AZURE_OPENAI_API_KEY" },
  { id: "cerebras", name: "Cerebras", envVar: "CEREBRAS_API_KEY" },
  { id: "huggingface", name: "Hugging Face", envVar: "HF_TOKEN" },
  { id: "cloudflare-workers-ai", name: "Cloudflare", envVar: "CLOUDFLARE_API_KEY" },
];

/// Recognizable key prefixes, keyed by the provider they actually belong to.
/// Used to catch a key pasted into the wrong provider's slot BEFORE it's
/// saved — the alternative (guessing the right provider from the key shape at
/// request time, which a previous version of this app did for OpenRouter) is
/// exactly the kind of silent model-to-key mismatch this is meant to prevent.
/// ponytail: heuristic prefix list, not exhaustive — many providers (Google,
/// Mistral, Cerebras, HF's "hf_" aside) don't have a single deterministic
/// prefix, so this only warns for the shapes we can actually recognize; it
/// never blocks saving.
const KEY_PREFIX_OWNER: Record<string, string> = {
  "sk-ant-": "anthropic",
  "sk-or-v1-": "openrouter",
  "sk-proj-": "openai",
  "gsk_": "groq",
  "hf_": "huggingface",
  "xai-": "xai",
};

function detectKeyMismatch(providerId: string, key: string): string | null {
  for (const [prefix, owner] of Object.entries(KEY_PREFIX_OWNER)) {
    if (key.startsWith(prefix) && owner !== providerId) {
      const ownerName = PROVIDERS.find((p) => p.id === owner)?.name ?? owner;
      return `This looks like a ${ownerName} key, not one for this provider — double check before saving.`;
    }
  }
  return null;
}

export const ApiKeyManager = ({ inline = false }: { inline?: boolean }) => {
  const { storeApiKey, deleteApiKey, authenticatedProviders } = useAgent();
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [showKey, setShowKey] = useState<Record<string, boolean>>({});
  const [saving, setSaving] = useState<Record<string, boolean>>({});
  const [saved, setSaved] = useState<Record<string, boolean>>({});

  const handleSave = async (provider: ProviderConfig) => {
    const key = apiKeys[provider.id]?.trim();
    if (!key) return;

    setSaving((p) => ({ ...p, [provider.id]: true }));
    await storeApiKey(provider.id, key);
    setSaving((p) => ({ ...p, [provider.id]: false }));
    setSaved((p) => ({ ...p, [provider.id]: true }));
    setTimeout(() => setSaved((p) => ({ ...p, [provider.id]: false })), 2000);
  };

  const handleDelete = async (providerId: string) => {
    await deleteApiKey(providerId);
    setApiKeys((prev) => {
      const n = { ...prev };
      delete n[providerId];
      return n;
    });
  };

  return (
    <div className={cn("w-full rounded-xl flex flex-col", inline ? "bg-transparent" : "bg-canvas/50 backdrop-blur-xl border border-hairline/60 max-h-[85vh] max-w-[560px]")}>
      {!inline && (
        <div className="flex items-center gap-3 px-6 py-5 border-b border-hairline">
          <div className="w-8 h-8 rounded-full bg-primary flex items-center justify-center">
            <HugeiconsIcon icon={Key01Icon} size={16} className="text-on-primary" />
          </div>
          <h2 className="text-lg font-medium font-sans text-ink">API Keys</h2>
        </div>
      )}

      <div className="flex-1 overflow-y-auto space-y-4">
        {!inline && (
          <p className="text-sm text-body font-sans leading-relaxed">
            Add provider API keys to enable models. Keys are stored securely in the app data directory.
          </p>
        )}

        {PROVIDERS.map((provider) => {
          const isAuthed = authenticatedProviders.has(provider.id);
          return (
            <div
              key={provider.id}
              className={cn("p-4 border rounded-xl bg-canvas/40 backdrop-blur-md transition-colors", isAuthed ? "border-ink/60" : "border-hairline/60")}
            >
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <ProviderIcon provider={provider.id} size={16} className="text-ink flex-shrink-0" />
                  <span className="text-sm font-medium text-ink font-sans">{provider.name}</span>
                  {isAuthed && <HugeiconsIcon icon={Tick02Icon} size={14} className="text-emerald-600" />}
                </div>
                {isAuthed && (
                  <Button variant="ghost" size="icon-sm" onClick={() => handleDelete(provider.id)} className="text-mute hover:text-red-500">
                    <HugeiconsIcon icon={Delete02Icon} size={14} />
                  </Button>
                )}
              </div>

              <div className="relative">
                <Input
                  type={showKey[provider.id] ? "text" : "password"}
                  value={apiKeys[provider.id] || ""}
                  onChange={(e) => setApiKeys((prev) => ({ ...prev, [provider.id]: e.target.value }))}
                  placeholder={isAuthed ? "Key saved" : `Enter ${provider.envVar}`}
                  className="font-mono text-[13px] pr-11"
                />
                <button
                  onClick={() => setShowKey((p) => ({ ...p, [provider.id]: !p[provider.id] }))}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 hover:bg-surface-soft rounded-full text-body transition-colors"
                >
                  <HugeiconsIcon icon={showKey[provider.id] ? ViewOffSlashIcon : ViewIcon} size={14} />
                </button>
              </div>

              {(() => {
                const key = apiKeys[provider.id]?.trim();
                const mismatch = key ? detectKeyMismatch(provider.id, key) : null;
                if (!mismatch) return null;
                return (
                  <div className="mt-1.5 flex items-center gap-1.5 text-amber-600">
                    <HugeiconsIcon icon={Alert02Icon} size={12} className="flex-shrink-0" />
                    <span className="text-[11px] font-sans">{mismatch}</span>
                  </div>
                );
              })()}

              <div className="mt-2 flex items-center justify-between">
                <span className="text-xs text-mute font-sans">{provider.envVar}</span>
                <Button
                  size="sm"
                  onClick={() => handleSave(provider)}
                  disabled={!apiKeys[provider.id]?.trim() || saving[provider.id]}
                  className={cn(saved[provider.id] && "bg-emerald-500 hover:bg-emerald-600")}
                >
                  {saving[provider.id] ? "Saving..." : saved[provider.id] ? "Saved!" : "Save"}
                </Button>
              </div>
            </div>
          );
        })}
      </div>

      {!inline && (
        <div className="px-6 py-4 border-t border-hairline bg-surface-soft flex items-center gap-2">
          <HugeiconsIcon icon={Alert02Icon} size={14} className="text-body flex-shrink-0" />
          <p className="text-xs text-body font-sans">
            API keys are stored locally and never sent to our servers. They are only used for direct provider calls.
          </p>
        </div>
      )}
    </div>
  );
};
