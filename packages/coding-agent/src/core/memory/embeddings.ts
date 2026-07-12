/**
 * Text embedding generation for episodic memory.
 *
 * Uses OpenAI's embeddings API when an API key is configured (real semantic
 * embeddings). Falls back to a deterministic local feature-hashing embedding
 * otherwise, so memory storage and retrieval remain functional - and fully
 * offline/testable - without any provider credentials.
 */

import type { ProviderEnv } from "@v3dqnt/vivianne-agent-ai";
import { getEnvApiKey } from "@v3dqnt/vivianne-agent-ai/compat";

export const EMBEDDING_DIM = 768;

export type EmbedFn = (text: string) => Promise<Float32Array>;

function normalize(vec: Float32Array): Float32Array {
	let sumSquares = 0;
	for (let i = 0; i < vec.length; i++) {
		sumSquares += vec[i] * vec[i];
	}
	const norm = Math.sqrt(sumSquares);
	if (norm === 0) return vec;
	for (let i = 0; i < vec.length; i++) {
		vec[i] /= norm;
	}
	return vec;
}

// FNV-1a, good enough distribution for feature hashing (not cryptographic).
function hashToken(token: string): number {
	let hash = 0x811c9dc5;
	for (let i = 0; i < token.length; i++) {
		hash ^= token.charCodeAt(i);
		hash = Math.imul(hash, 0x01000193);
	}
	return hash >>> 0;
}

/**
 * Deterministic bag-of-words feature-hashing embedding.
 *
 * Not a learned semantic embedding, but content-sensitive (unlike a constant
 * vector) and stable across processes/machines, so cosine/L2 similarity over
 * these vectors still surfaces lexically related memories without any
 * network calls or model downloads.
 */
export function createLocalHashEmbedding(text: string): Float32Array {
	const vec = new Float32Array(EMBEDDING_DIM);
	const tokens = text.toLowerCase().match(/[a-z0-9]+/g) ?? [];
	for (const token of tokens) {
		const hash = hashToken(token);
		const index = hash % EMBEDDING_DIM;
		const sign = hash & 0x10000 ? 1 : -1;
		vec[index] += sign;
	}
	return normalize(vec);
}

async function embedWithOpenAI(text: string, apiKey: string): Promise<Float32Array> {
	const response = await fetch("https://api.openai.com/v1/embeddings", {
		method: "POST",
		headers: {
			"content-type": "application/json",
			authorization: `Bearer ${apiKey}`,
		},
		body: JSON.stringify({
			model: "text-embedding-3-small",
			input: text,
			dimensions: EMBEDDING_DIM,
		}),
	});
	if (!response.ok) {
		throw new Error(`OpenAI embeddings request failed: ${response.status} ${await response.text()}`);
	}
	const data = (await response.json()) as { data?: Array<{ embedding?: number[] }> };
	const embedding = data.data?.[0]?.embedding;
	if (!Array.isArray(embedding) || embedding.length !== EMBEDDING_DIM) {
		throw new Error("OpenAI embeddings response did not contain a valid embedding vector");
	}
	return normalize(Float32Array.from(embedding));
}

/**
 * Build the embedding function memory storage/retrieval should use.
 *
 * Prefers a real OpenAI embedding when an API key is available in the
 * environment; transparently falls back to the local hashing embedding for
 * that call if the request fails (offline, rate-limited, revoked key, etc.)
 * so memory never becomes unavailable due to a network error.
 */
export function createEmbedFn(env?: ProviderEnv): EmbedFn {
	const apiKey = getEnvApiKey("openai", env);
	if (!apiKey) {
		return async (text: string) => createLocalHashEmbedding(text);
	}
	return async (text: string) => {
		try {
			return await embedWithOpenAI(text, apiKey);
		} catch {
			return createLocalHashEmbedding(text);
		}
	};
}
