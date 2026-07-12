import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import * as sqliteVec from "sqlite-vec";
import { EMBEDDING_DIM } from "./embeddings.ts";

export type EpisodeSource = "episode" | "fact";

export interface EpisodeRow {
	id: number;
	content: string;
	timestamp: string;
	sessionId: string;
	accessCount: number;
	weight: number;
	source: EpisodeSource;
}

export interface EpisodeMatch extends EpisodeRow {
	/** Vector distance to the query embedding (lower is closer). */
	distance: number;
}

export class SqliteVecAdapter {
	private db: Database.Database;

	constructor(dbPath: string) {
		const dir = path.dirname(dbPath);
		if (!fs.existsSync(dir)) {
			fs.mkdirSync(dir, { recursive: true });
		}
		this.db = new Database(dbPath);

		// Load the sqlite-vec extension
		sqliteVec.load(this.db);

		this.initialize();
	}

	private initialize() {
		this.db.exec(`
			CREATE TABLE IF NOT EXISTS episodic_memory (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				content TEXT,
				timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
				session_id TEXT,
				access_count INTEGER DEFAULT 0,
				weight REAL DEFAULT 1.0,
				source TEXT DEFAULT 'episode'
			);
			CREATE VIRTUAL TABLE IF NOT EXISTS episodic_memory_vec USING vec0(
				embedding float[${EMBEDDING_DIM}]
			);
		`);
		this.migrateAddSourceColumn();
	}

	// Older databases (written before source-tagging was added) won't have this column.
	private migrateAddSourceColumn(): void {
		const columns = this.db.prepare("PRAGMA table_info(episodic_memory)").all() as Array<{ name: string }>;
		if (!columns.some((column) => column.name === "source")) {
			this.db.exec("ALTER TABLE episodic_memory ADD COLUMN source TEXT DEFAULT 'episode'");
		}
	}

	public insertEpisode(
		embedding: Float32Array,
		content: string,
		sessionId: string,
		weight = 1.0,
		source: EpisodeSource = "episode",
	) {
		const stmt = this.db.prepare(
			"INSERT INTO episodic_memory (content, session_id, weight, source) VALUES (?, ?, ?, ?)",
		);
		const result = stmt.run(content, sessionId, weight, source);

		const vecStmt = this.db.prepare("INSERT INTO episodic_memory_vec (rowid, embedding) VALUES (?, ?)");
		// vec0 virtual tables only accept a bound rowid parameter as a BigInt - a plain JS
		// number binds with the wrong SQLite type affinity and vec0 rejects it as a primary key.
		vecStmt.run(BigInt(result.lastInsertRowid), new Uint8Array(embedding.buffer));
	}

	/** K-nearest-neighbor search over stored episodes, closest first. */
	public queryNearest(embedding: Float32Array, limit: number): EpisodeMatch[] {
		if (limit <= 0) return [];
		const stmt = this.db.prepare(`
			SELECT
				e.id AS id,
				e.content AS content,
				e.timestamp AS timestamp,
				e.session_id AS sessionId,
				e.access_count AS accessCount,
				e.weight AS weight,
				e.source AS source,
				v.distance AS distance
			FROM episodic_memory_vec v
			JOIN episodic_memory e ON e.id = v.rowid
			WHERE v.embedding MATCH ? AND k = ?
			ORDER BY v.distance
		`);
		return stmt.all(new Uint8Array(embedding.buffer), limit) as EpisodeMatch[];
	}

	public incrementAccessCount(ids: number[]): void {
		if (ids.length === 0) return;
		const stmt = this.db.prepare("UPDATE episodic_memory SET access_count = access_count + 1 WHERE id = ?");
		const tx = this.db.transaction((idList: number[]) => {
			for (const id of idList) stmt.run(id);
		});
		tx(ids);
	}

	public close() {
		this.db.close();
	}
}
