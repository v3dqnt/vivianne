import { SqliteVecAdapter } from "./sqlite-vec-adapter.ts";

export interface MemoryItem {
	content: string;
	timestamp: Date;
	accessCount: number;
	weight: number;
}

export class MemoryManager {
	private adapter?: SqliteVecAdapter;

	constructor(dbPath?: string) {
		if (dbPath) {
			this.adapter = new SqliteVecAdapter(dbPath);
		}
	}

	public async extractAndStoreEpisode(content: string, sessionId: string) {
		if (!this.adapter) return;
		
		// In a real implementation, you would use a local fast embedding model here
		// e.g. using @xenova/transformers or passing it to the configured LLM API.
		// For now, we mock an embedding vector of dimension 768.
		const mockEmbedding = new Float32Array(768).fill(0.1); 
		
		this.adapter.insertEpisode(mockEmbedding, content, sessionId);
	}

	public calculateMemoryScore(item: MemoryItem, currentTime: Date): number {
		const baseWeight = item.weight;
		const ageDays = (currentTime.getTime() - item.timestamp.getTime()) / (1000 * 60 * 60 * 24);
		const decayFactor = Math.pow(0.95, ageDays); // 5% decay per day
		const frequencyBoost = Math.log1p(item.accessCount); // log1p(x) = ln(1 + x)
		return baseWeight * frequencyBoost * decayFactor;
	}

	public close() {
		if (this.adapter) {
			this.adapter.close();
		}
	}
}
