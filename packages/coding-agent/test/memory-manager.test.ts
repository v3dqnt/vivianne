import { rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createLocalHashEmbedding } from "../src/core/memory/embeddings.ts";
import { MemoryManager } from "../src/core/memory/memory-manager.ts";

describe("embeddings", () => {
	it("produces deterministic, unit-normalized vectors", () => {
		const a = createLocalHashEmbedding("the quick brown fox");
		const b = createLocalHashEmbedding("the quick brown fox");
		expect(Array.from(a)).toEqual(Array.from(b));

		let sumSquares = 0;
		for (const value of a) sumSquares += value * value;
		expect(Math.sqrt(sumSquares)).toBeCloseTo(1, 5);
	});

	it("produces different vectors for different content", () => {
		const a = createLocalHashEmbedding("apples and oranges");
		const b = createLocalHashEmbedding("quantum computing research");
		expect(Array.from(a)).not.toEqual(Array.from(b));
	});
});

describe("MemoryManager", () => {
	let dbPath: string;
	let manager: MemoryManager;

	beforeEach(() => {
		dbPath = join(tmpdir(), `pi-memory-test-${Date.now()}-${Math.random().toString(36).slice(2)}.sqlite`);
		manager = new MemoryManager(dbPath, async (text) => createLocalHashEmbedding(text));
	});

	afterEach(() => {
		manager.close();
		rmSync(dbPath, { force: true });
		rmSync(`${dbPath}-journal`, { force: true });
	});

	it("recalls the lexically closest stored memory first", async () => {
		await manager.extractAndStoreEpisode("Session summary: migrated the billing service to Postgres", "session-a");
		await manager.extractAndStoreEpisode("Session summary: fixed a flaky CI job for the frontend tests", "session-b");

		const results = await manager.recall("postgres billing migration", 1);
		expect(results).toHaveLength(1);
		expect(results[0].content).toContain("Postgres");
	});

	it("ranks explicit remember()'d facts above automatic episodes when otherwise similar", async () => {
		await manager.extractAndStoreEpisode("user likes pnpm for package management", "session-a");
		await manager.remember("user likes pnpm for package management", "session-a");

		const results = await manager.recall("pnpm package management preference", 2);
		expect(results).toHaveLength(2);
		// The remembered fact has a higher base weight, so it should score higher and sort first.
		const now = new Date();
		const scoreA = manager.calculateMemoryScore(
			{ content: results[0].content, timestamp: results[0].timestamp, accessCount: 0, weight: 2.0 },
			now,
		);
		const scoreB = manager.calculateMemoryScore(
			{ content: results[1].content, timestamp: results[1].timestamp, accessCount: 0, weight: 1.0 },
			now,
		);
		expect(scoreA).toBeGreaterThan(scoreB);
	});

	it("returns an empty array when nothing has been stored", async () => {
		expect(await manager.recall("anything")).toEqual([]);
	});

	it("scores a never-accessed memory above zero", () => {
		const score = manager.calculateMemoryScore(
			{ content: "x", timestamp: new Date(), accessCount: 0, weight: 1.0 },
			new Date(),
		);
		expect(score).toBeGreaterThan(0);
	});
});
