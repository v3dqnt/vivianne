import type { ContextBudget } from "./context-budget.ts";

export interface ContextChunk {
	source: string;
	timestamp: string;
	text: string;
}

export class ContextManager {
	private chunks: ContextChunk[] = [];

	public addSemanticChunk(chunk: ContextChunk) {
		this.chunks.push(chunk);
	}

	public getRetrievedSemanticPreferences(): string {
		// Mock implementation for semantic preferences
		return "The user prefers concise answers.";
	}

	public getRetrievedChunks(budget: ContextBudget): ContextChunk[] {
		// In a real scenario, this would rank and trim based on budget.retrievalTokens
		return this.chunks;
	}

	public clearChunks() {
		this.chunks = [];
	}
}
