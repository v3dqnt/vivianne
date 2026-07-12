"use client";

import React from "react";

/**
 * Simplified, original monochrome marks per provider — not reproductions of
 * any provider's registered trademark artwork, just a distinct recognizable
 * glyph per brand so the model picker/API key list reads at a glance instead
 * of via emoji. All render via `currentColor` so they follow text color.
 */

type IconProps = { size?: number; className?: string };

const base = (size: number) => ({ width: size, height: size, viewBox: "0 0 24 24" });

export function AnthropicIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M14.5 4h-3.2L5 20h3.3l1.2-3.3h6l1.2 3.3H20L14.5 4zm-3.9 9.7L12.9 7.5l2.3 6.2h-4.6z" fill="currentColor" />
    </svg>
  );
}

export function OpenAIIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M12 2.5a6.7 6.7 0 0 0-6.4 4.7A6.2 6.2 0 0 0 2 12.7a6.7 6.7 0 0 0 3.3 8 6.2 6.2 0 0 0 2.3 5.3 6.7 6.7 0 0 0 7.4-1 6.2 6.2 0 0 0 5.3-2.3 6.7 6.7 0 0 0-1-9.5 6.2 6.2 0 0 0-1-6.4A6.7 6.7 0 0 0 12 2.5zm0 3.2c1.2 0 2.2.5 3 1.3l-4.8 2.8a1 1 0 0 0-.5.9v6.4l-2-1.1V9.3A5 5 0 0 1 12 5.7zm5.4 3.6a5 5 0 0 1 .5 4.3l-4.8-2.8v-2.3l3.6-2a5 5 0 0 1 .7 2.8zM7.3 9.6l4.7 2.7v2.3l-3.6 2a5 5 0 0 1-3.1-6.7l2-.3zm.7 8.8 4.8-2.8a1 1 0 0 0 .5-.9V8.3l2 1.1v6.4a5 5 0 0 1-7.3 2.6z"
        fill="currentColor"
      />
    </svg>
  );
}

export function GoogleIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} xmlns="http://www.w3.org/2000/svg">
      <path d="M21.6 12.2c0-.7-.06-1.4-.18-2H12v3.9h5.4a4.6 4.6 0 0 1-2 3v2.5h3.2c1.9-1.7 3-4.3 3-7.4z" fill="#4285F4" />
      <path d="M12 22c2.7 0 5-.9 6.6-2.4l-3.2-2.5c-.9.6-2 1-3.4 1-2.6 0-4.9-1.8-5.7-4.2H3v2.6A10 10 0 0 0 12 22z" fill="#34A853" />
      <path d="M6.3 13.9a6 6 0 0 1 0-3.8V7.5H3a10 10 0 0 0 0 9l3.3-2.6z" fill="#FBBC05" />
      <path d="M12 6c1.5 0 2.8.5 3.8 1.5l2.8-2.8C16.9 3 14.7 2 12 2A10 10 0 0 0 3 7.5l3.3 2.6C7.1 7.8 9.4 6 12 6z" fill="#EA4335" />
    </svg>
  );
}

export function MistralIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} xmlns="http://www.w3.org/2000/svg">
      <g fill="currentColor">
        <rect x="3" y="4" width="4" height="4" />
        <rect x="9" y="4" width="4" height="4" opacity="0.7" />
        <rect x="15" y="4" width="4" height="4" opacity="0.4" />
        <rect x="3" y="10" width="4" height="4" opacity="0.85" />
        <rect x="9" y="10" width="4" height="4" opacity="0.55" />
        <rect x="15" y="10" width="4" height="4" opacity="0.25" />
        <rect x="3" y="16" width="4" height="4" opacity="0.7" />
        <rect x="9" y="16" width="4" height="4" opacity="0.4" />
        <rect x="15" y="16" width="4" height="4" opacity="0.15" />
      </g>
    </svg>
  );
}

export function XAIIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M4 4h4.2l4 5.6L16.2 4H20l-6 8 6.4 8h-4.2l-4.3-6-4.3 6H3.4l6.2-8L4 4z" fill="currentColor" />
    </svg>
  );
}

export function DeepSeekIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.6" />
      <path d="M8 9.5c1.5-1.8 6.5-1.8 8 0M8 14.5c1.5 1.8 6.5 1.8 8 0" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

export function GroqIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" />
      <path d="M12 8v8M8.5 10l7 4M15.5 10l-7 4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

export function TogetherIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="9" cy="9" r="5" fill="currentColor" opacity="0.6" />
      <circle cx="15" cy="15" r="5" fill="currentColor" opacity="0.6" />
    </svg>
  );
}

export function FireworksIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M12 2v4M12 18v4M2 12h4M18 12h4M5 5l2.8 2.8M16.2 16.2 19 19M19 5l-2.8 2.8M7.8 16.2 5 19" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <circle cx="12" cy="12" r="3" fill="currentColor" />
    </svg>
  );
}

export function OpenRouterIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="5" cy="12" r="2.2" fill="currentColor" />
      <circle cx="19" cy="6" r="2.2" fill="currentColor" />
      <circle cx="19" cy="18" r="2.2" fill="currentColor" />
      <path d="M7 12h5M12 12l5-5.3M12 12l5 5.3" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}

export function CerebrasIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M12 3a9 9 0 1 0 9 9" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <path d="M12 7a5 5 0 1 0 5 5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      <circle cx="12" cy="12" r="1.6" fill="currentColor" />
    </svg>
  );
}

export function NvidiaIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M3 15c4-6 14-6 18 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      <path d="M3 15h18" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" opacity="0.5" />
      <circle cx="12" cy="8" r="1.6" fill="currentColor" />
    </svg>
  );
}

export function AzureIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M9 3h6l-4.5 13H15l-8.5 5 4-9H6L9 3z" fill="currentColor" />
    </svg>
  );
}

export function HuggingFaceIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="12" cy="12" r="8" stroke="currentColor" strokeWidth="1.6" />
      <circle cx="9" cy="10" r="1" fill="currentColor" />
      <circle cx="15" cy="10" r="1" fill="currentColor" />
      <path d="M8 14c1.3 1.3 6.7 1.3 8 0" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

export function CloudflareIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M4 16c0-2.2 1.8-4 4-4 .3-2.3 2.3-4 4.6-4 2 0 3.7 1.2 4.4 3a3.5 3.5 0 0 1 4 3.4c0 .6-.1 1.1-.4 1.6H4z" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    </svg>
  );
}

export function GenericProviderIcon({ size = 16, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} fill="none" xmlns="http://www.w3.org/2000/svg">
      <rect x="4" y="4" width="16" height="16" rx="4" stroke="currentColor" strokeWidth="1.6" />
      <circle cx="12" cy="12" r="3" fill="currentColor" />
    </svg>
  );
}

const REGISTRY: Record<string, React.ComponentType<IconProps>> = {
  anthropic: AnthropicIcon,
  openai: OpenAIIcon,
  google: GoogleIcon,
  mistral: MistralIcon,
  xai: XAIIcon,
  deepseek: DeepSeekIcon,
  groq: GroqIcon,
  together: TogetherIcon,
  fireworks: FireworksIcon,
  openrouter: OpenRouterIcon,
  cerebras: CerebrasIcon,
  nvidia: NvidiaIcon,
  "nvidia-nim": NvidiaIcon,
  "azure-openai-responses": AzureIcon,
  huggingface: HuggingFaceIcon,
  "cloudflare-workers-ai": CloudflareIcon,
};

export function ProviderIcon({ provider, size = 16, className }: { provider: string } & IconProps) {
  const Icon = REGISTRY[provider] ?? GenericProviderIcon;
  return <Icon size={size} className={className} />;
}
