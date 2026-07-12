import { createEmbedFn, type EmbedFn } from "./embeddings.ts";
import { SqliteVecAdapter } from "./sqlite-vec-adapter.ts";

export interface MemoryItem {
	content: string;
	timestamp: Date;
	accessCount: number;
	weight: number;
}

const RECALL_CANDIDATE_MULTIPLIER = 4;

export class MemoryManager {
	private adapter?: SqliteVecAdapter;
	private embed: EmbedFn;

	constructor(dbPath?: string, embed?: EmbedFn) {
		if (dbPath) {
			this.adapter = new SqliteVecAdapter(dbPath);
		}
		this.embed = embed ?? createEmbedFn();
	}

	/** Automatic capture: store a post-compaction session summary as an episodic memory. */
	public async extractAndStoreEpisode(content: string, sessionId: string): Promise<void> {
		if (!this.adapter) return;
		const embedding = await this.embed(content);
		this.adapter.insertEpisode(embedding, content, sessionId, 1.0, "episode");
	}

	/** Explicit capture: store a durable fact/preference/constraint the agent was told to remember. */
	public async remember(content: string, sessionId: string): Promise<void> {
		if (!this.adapter) return;
		const embedding = await this.embed(content);
		// Explicit facts are weighted higher than automatic episode summaries so they
		// outrank incidental session chatter when scores are close.
		this.adapter.insertEpisode(embedding, content, sessionId, 2.0, "fact");
	}

	/** Semantic search over stored memories, ranked by similarity, recency, and access frequency. */
	public async recall(query: string, limit = 5): Promise<MemoryItem[]> {
		if (!this.adapter || limit <= 0) return [];
		const embedding = await this.embed(query);
		const candidates = this.adapter.queryNearest(embedding, limit * RECALL_CANDIDATE_MULTIPLIER);
		if (candidates.length === 0) return [];

		const now = new Date();
		const scored = candidates.map((candidate) => {
			const item: MemoryItem = {
				content: candidate.content,
				timestamp: new Date(candidate.timestamp),
				accessCount: candidate.accessCount,
				weight: candidate.weight,
			};
			// Lower vector distance = more similar; fold it in as a multiplicative similarity factor
			// alongside the existing recency/frequency decay score.
			const similarity = 1 / (1 + candidate.distance);
			return { id: candidate.id, item, score: this.calculateMemoryScore(item, now) * similarity };
		});

		scored.sort((a, b) => b.score - a.score);
		const top = scored.slice(0, limit);
		this.adapter.incrementAccessCount(top.map((entry) => entry.id));
		return top.map((entry) => entry.item);
	}

	public calculateMemoryScore(item: MemoryItem, currentTime: Date): number {
		const baseWeight = item.weight;
		const ageDays = (currentTime.getTime() - item.timestamp.getTime()) / (1000 * 60 * 60 * 24);
		const decayFactor = 0.95 ** ageDays; // 5% decay per day
		// 1 + log1p(accessCount) so a never-accessed memory still scores baseWeight * decayFactor
		// instead of always ranking last (log1p(0) === 0 would zero out every new memory).
		const frequencyBoost = 1 + Math.log1p(item.accessCount);
		return baseWeight * frequencyBoost * decayFactor;
	}

	public close() {
		if (this.adapter) {
			this.adapter.close();
		}
	}
}
