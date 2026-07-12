import type { AgentTool } from "@v3dqnt/vivianne-agent-agent-core";
import { type Static, Type } from "typebox";
import type { ToolDefinition } from "../extensions/types.ts";
import { wrapToolDefinition } from "./tool-definition-wrapper.ts";

const recallSchema = Type.Object({
	query: Type.String({ description: "What to recall - a topic, project, or question about prior context." }),
	limit: Type.Optional(Type.Number({ description: "Maximum number of memories to return. Default 5." })),
});

export type RecallToolInput = Static<typeof recallSchema>;

export interface RecalledMemory {
	content: string;
	timestamp: Date;
}

export interface RecallToolOptions {
	onRecall?: (query: string, limit?: number) => Promise<RecalledMemory[]>;
}

export function createRecallToolDefinition(
	options?: RecallToolOptions,
): ToolDefinition<typeof recallSchema, undefined> {
	return {
		name: "recall",
		label: "recall",
		description:
			"Search long-term memory for facts, preferences, or past session summaries relevant to a query. Use at the start of a task that plausibly touches prior context - a project you've seen before, stated user preferences.",
		promptSnippet: "Search long-term memory for relevant past context",
		parameters: recallSchema,
		async execute(_toolCallId, { query, limit }: RecallToolInput, _signal, _onUpdate, _ctx) {
			if (!options?.onRecall) {
				return {
					content: [{ type: "text", text: "No memories available in this environment." }],
					details: undefined,
				};
			}
			const items = await options.onRecall(query, limit);
			if (items.length === 0) {
				return { content: [{ type: "text", text: "No relevant memories found." }], details: undefined };
			}
			const text = items
				.map((item, i) => `${i + 1}. [${item.timestamp.toISOString().slice(0, 10)}] ${item.content}`)
				.join("\n");
			return { content: [{ type: "text", text }], details: undefined };
		},
	};
}

export function createRecallTool(options?: RecallToolOptions): AgentTool<typeof recallSchema> {
	return wrapToolDefinition(createRecallToolDefinition(options));
}
