import type { AgentTool } from "@v3dqnt/vivianne-agent-agent-core";
import { type Static, Type } from "typebox";
import type { ToolDefinition } from "../extensions/types.ts";
import { wrapToolDefinition } from "./tool-definition-wrapper.ts";

const rememberSchema = Type.Object({
	content: Type.String({
		description:
			"A durable fact, correction, or preference to store in long-term memory, phrased so it stands alone without today's conversation as context.",
	}),
});

export type RememberToolInput = Static<typeof rememberSchema>;

export interface RememberToolOptions {
	onRemember?: (content: string) => Promise<void>;
}

export function createRememberToolDefinition(
	options?: RememberToolOptions,
): ToolDefinition<typeof rememberSchema, undefined> {
	return {
		name: "remember",
		label: "remember",
		description:
			"Store a durable fact in long-term memory so it's available in future sessions: user preferences ('always use pnpm'), project constraints, corrections you were given. Don't store what the repo already records or what only matters for this session.",
		promptSnippet: "Store a durable fact or preference in long-term memory",
		parameters: rememberSchema,
		async execute(_toolCallId, { content }: RememberToolInput, _signal, _onUpdate, _ctx) {
			if (!options?.onRemember) {
				return {
					content: [{ type: "text", text: "Memory storage is not available in this environment." }],
					details: undefined,
				};
			}
			await options.onRemember(content);
			return { content: [{ type: "text", text: `Remembered: ${content}` }], details: undefined };
		},
	};
}

export function createRememberTool(options?: RememberToolOptions): AgentTool<typeof rememberSchema> {
	return wrapToolDefinition(createRememberToolDefinition(options));
}
