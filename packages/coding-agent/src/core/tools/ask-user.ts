import type { AgentTool } from "@v3dqnt/vivianne-agent-agent-core";
import { Type, type Static } from "typebox";
import type { ToolDefinition } from "../extensions/types.ts";
import { wrapToolDefinition } from "./tool-definition-wrapper.ts";

const askUserSchema = Type.Object({
	question: Type.String({ description: "The question to ask the user." }),
	recommendations: Type.Optional(Type.Array(Type.String(), { description: "Optional alternate recommendations or choices for the user." }))
});

export type AskUserToolInput = Static<typeof askUserSchema>;

export interface AskUserToolOptions {
	askUserHook?: (question: string, recommendations?: string[]) => Promise<string>;
}

export function createAskUserToolDefinition(options?: AskUserToolOptions): ToolDefinition<typeof askUserSchema, string> {
	return {
		name: "ask_user",
		label: "ask_user",
		description: "Ask the user a question to clarify requirements, solicit feedback, or provide alternate recommendations. Execution will pause until the user answers.",
		parameters: askUserSchema,
		execute: async (_toolCallId, input: AskUserToolInput, signal, onUpdate, ctx) => {
			if (!options?.askUserHook) {
				return { content: [{ type: "text", text: "The ask_user tool is not fully supported in this environment because the UI hook is not provided. Please output your question directly in text instead." }], details: "" };
			}
			try {
				const response = await options.askUserHook(input.question, input.recommendations);
				return { content: [{ type: "text", text: response }], details: "" };
			} catch (err: any) {
				return { content: [{ type: "text", text: `Failed to get user response: ${err.message}` }], details: "" };
			}
		},
	};
}

export function createAskUserTool(options?: AskUserToolOptions): AgentTool<AskUserToolInput> {
	return wrapToolDefinition(createAskUserToolDefinition(options));
}
