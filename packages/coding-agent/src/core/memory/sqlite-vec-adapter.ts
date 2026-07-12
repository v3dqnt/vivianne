import Database from "better-sqlite3";
import * as sqliteVec from "sqlite-vec";
import path from "node:path";
import fs from "node:fs";

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
				weight REAL DEFAULT 1.0
			);
			CREATE VIRTUAL TABLE IF NOT EXISTS episodic_memory_vec USING vec0(
				embedding float[768]
			);
		`);
	}

	public insertEpisode(embedding: Float32Array, content: string, sessionId: string, weight = 1.0) {
		const stmt = this.db.prepare(
			`INSERT INTO episodic_memory (content, session_id, weight) VALUES (?, ?, ?)`
		);
		const result = stmt.run(content, sessionId, weight);
		
		const vecStmt = this.db.prepare(
			`INSERT INTO episodic_memory_vec (rowid, embedding) VALUES (?, ?)`
		);
		vecStmt.run(result.lastInsertRowid, new Uint8Array(embedding.buffer));
	}

	public close() {
		this.db.close();
	}
}
