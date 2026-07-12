export {
	type AskUserToolInput,
	type AskUserToolOptions,
	createAskUserTool,
	createAskUserToolDefinition,
} from "./ask-user.ts";
export {
	type BashOperations,
	type BashSpawnContext,
	type BashSpawnHook,
	type BashToolDetails,
	type BashToolInput,
	type BashToolOptions,
	createBashTool,
	createBashToolDefinition,
	createLocalBashOperations,
} from "./bash.ts";
export {
	type ComputerToolInput,
	type ComputerToolOptions,
	createComputerTool,
	createComputerToolDefinition,
} from "./computer.ts";
export {
	createEditTool,
	createEditToolDefinition,
	type EditOperations,
	type EditToolDetails,
	type EditToolInput,
	type EditToolOptions,
} from "./edit.ts";
export { withFileMutationQueue } from "./file-mutation-queue.ts";
export {
	createFindTool,
	createFindToolDefinition,
	type FindOperations,
	type FindToolDetails,
	type FindToolInput,
	type FindToolOptions,
} from "./find.ts";
export {
	createGrepTool,
	createGrepToolDefinition,
	type GrepOperations,
	type GrepToolDetails,
	type GrepToolInput,
	type GrepToolOptions,
} from "./grep.ts";
export {
	createLsTool,
	createLsToolDefinition,
	type LsOperations,
	type LsToolDetails,
	type LsToolInput,
	type LsToolOptions,
} from "./ls.ts";
export {
	createReadTool,
	createReadToolDefinition,
	type ReadOperations,
	type ReadToolDetails,
	type ReadToolInput,
	type ReadToolOptions,
} from "./read.ts";
export {
	createRecallTool,
	createRecallToolDefinition,
	type RecalledMemory,
	type RecallToolInput,
	type RecallToolOptions,
} from "./recall.ts";
export {
	createRememberTool,
	createRememberToolDefinition,
	type RememberToolInput,
	type RememberToolOptions,
} from "./remember.ts";
export {
	createTodoWriteTool,
	createTodoWriteToolDefinition,
	type TodoItem,
	TodoListStore,
	type TodoWriteToolInput,
	type TodoWriteToolOptions,
} from "./todo.ts";
export {
	DEFAULT_MAX_BYTES,
	DEFAULT_MAX_LINES,
	formatSize,
	type TruncationOptions,
	type TruncationResult,
	truncateHead,
	truncateLine,
	truncateTail,
} from "./truncate.ts";
export {
	createWriteTool,
	createWriteToolDefinition,
	type WriteOperations,
	type WriteToolInput,
	type WriteToolOptions,
} from "./write.ts";

import type { AgentTool } from "@v3dqnt/vivianne-agent-agent-core";
import type { ToolDefinition } from "../extensions/types.ts";
import { type AskUserToolOptions, createAskUserTool, createAskUserToolDefinition } from "./ask-user.ts";
import { type BashToolOptions, createBashTool, createBashToolDefinition } from "./bash.ts";
import { type ComputerToolOptions, createComputerTool, createComputerToolDefinition } from "./computer.ts";
import { createEditTool, createEditToolDefinition, type EditToolOptions } from "./edit.ts";
import { createFindTool, createFindToolDefinition, type FindToolOptions } from "./find.ts";
import { createGrepTool, createGrepToolDefinition, type GrepToolOptions } from "./grep.ts";
import { createLsTool, createLsToolDefinition, type LsToolOptions } from "./ls.ts";
import { createReadTool, createReadToolDefinition, type ReadToolOptions } from "./read.ts";
import { createRecallTool, createRecallToolDefinition, type RecallToolOptions } from "./recall.ts";
import { createRememberTool, createRememberToolDefinition, type RememberToolOptions } from "./remember.ts";
import { createTodoWriteTool, createTodoWriteToolDefinition, type TodoWriteToolOptions } from "./todo.ts";
import { createWriteTool, createWriteToolDefinition, type WriteToolOptions } from "./write.ts";

export type Tool = AgentTool<any>;
export type ToolDef = ToolDefinition<any, any>;
export type ToolName =
	| "read"
	| "bash"
	| "edit"
	| "write"
	| "grep"
	| "find"
	| "ls"
	| "computer"
	| "ask_user"
	| "todo_write"
	| "recall"
	| "remember";
export const allToolNames: Set<ToolName> = new Set([
	"read",
	"bash",
	"edit",
	"write",
	"grep",
	"find",
	"ls",
	"computer",
	"ask_user",
	"todo_write",
	"recall",
	"remember",
]);

export interface ToolsOptions {
	read?: ReadToolOptions;
	bash?: BashToolOptions;
	write?: WriteToolOptions;
	edit?: EditToolOptions;
	grep?: GrepToolOptions;
	find?: FindToolOptions;
	ls?: LsToolOptions;
	computer?: ComputerToolOptions;
	ask_user?: AskUserToolOptions;
	todo_write?: TodoWriteToolOptions;
	recall?: RecallToolOptions;
	remember?: RememberToolOptions;
}

export function createToolDefinition(toolName: ToolName, cwd: string, options?: ToolsOptions): ToolDef {
	switch (toolName) {
		case "read":
			return createReadToolDefinition(cwd, options?.read);
		case "bash":
			return createBashToolDefinition(cwd, options?.bash);
		case "edit":
			return createEditToolDefinition(cwd, options?.edit);
		case "write":
			return createWriteToolDefinition(cwd, options?.write);
		case "grep":
			return createGrepToolDefinition(cwd, options?.grep);
		case "find":
			return createFindToolDefinition(cwd, options?.find);
		case "ls":
			return createLsToolDefinition(cwd, options?.ls);
		case "computer":
			return createComputerToolDefinition(options?.computer);
		case "ask_user":
			return createAskUserToolDefinition(options?.ask_user);
		case "todo_write":
			return createTodoWriteToolDefinition(options?.todo_write);
		case "recall":
			return createRecallToolDefinition(options?.recall);
		case "remember":
			return createRememberToolDefinition(options?.remember);
		default:
			throw new Error(`Unknown tool name: ${toolName}`);
	}
}

export function createTool(toolName: ToolName, cwd: string, options?: ToolsOptions): Tool {
	switch (toolName) {
		case "read":
			return createReadTool(cwd, options?.read);
		case "bash":
			return createBashTool(cwd, options?.bash);
		case "edit":
			return createEditTool(cwd, options?.edit);
		case "write":
			return createWriteTool(cwd, options?.write);
		case "grep":
			return createGrepTool(cwd, options?.grep);
		case "find":
			return createFindTool(cwd, options?.find);
		case "ls":
			return createLsTool(cwd, options?.ls);
		case "computer":
			return createComputerTool(options?.computer);
		case "ask_user":
			return createAskUserTool(options?.ask_user);
		case "todo_write":
			return createTodoWriteTool(options?.todo_write);
		case "recall":
			return createRecallTool(options?.recall);
		case "remember":
			return createRememberTool(options?.remember);
		default:
			throw new Error(`Unknown tool name: ${toolName}`);
	}
}

export function createCodingToolDefinitions(cwd: string, options?: ToolsOptions): ToolDef[] {
	return [
		createReadToolDefinition(cwd, options?.read),
		createBashToolDefinition(cwd, options?.bash),
		createEditToolDefinition(cwd, options?.edit),
		createWriteToolDefinition(cwd, options?.write),
	];
}

export function createReadOnlyToolDefinitions(cwd: string, options?: ToolsOptions): ToolDef[] {
	return [
		createReadToolDefinition(cwd, options?.read),
		createGrepToolDefinition(cwd, options?.grep),
		createFindToolDefinition(cwd, options?.find),
		createLsToolDefinition(cwd, options?.ls),
	];
}

export function createAllToolDefinitions(cwd: string, options?: ToolsOptions): Record<ToolName, ToolDef> {
	return {
		read: createReadToolDefinition(cwd, options?.read),
		bash: createBashToolDefinition(cwd, options?.bash),
		edit: createEditToolDefinition(cwd, options?.edit),
		write: createWriteToolDefinition(cwd, options?.write),
		grep: createGrepToolDefinition(cwd, options?.grep),
		find: createFindToolDefinition(cwd, options?.find),
		ls: createLsToolDefinition(cwd, options?.ls),
		computer: createComputerToolDefinition(options?.computer),
		ask_user: createAskUserToolDefinition(options?.ask_user),
		todo_write: createTodoWriteToolDefinition(options?.todo_write),
		recall: createRecallToolDefinition(options?.recall),
		remember: createRememberToolDefinition(options?.remember),
	};
}

export function createCodingTools(cwd: string, options?: ToolsOptions): Tool[] {
	return [
		createReadTool(cwd, options?.read),
		createBashTool(cwd, options?.bash),
		createEditTool(cwd, options?.edit),
		createWriteTool(cwd, options?.write),
	];
}

export function createReadOnlyTools(cwd: string, options?: ToolsOptions): Tool[] {
	return [
		createReadTool(cwd, options?.read),
		createGrepTool(cwd, options?.grep),
		createFindTool(cwd, options?.find),
		createLsTool(cwd, options?.ls),
	];
}

export function createAllTools(cwd: string, options?: ToolsOptions): Record<ToolName, Tool> {
	return {
		read: createReadTool(cwd, options?.read),
		bash: createBashTool(cwd, options?.bash),
		edit: createEditTool(cwd, options?.edit),
		write: createWriteTool(cwd, options?.write),
		grep: createGrepTool(cwd, options?.grep),
		find: createFindTool(cwd, options?.find),
		ls: createLsTool(cwd, options?.ls),
		computer: createComputerTool(options?.computer),
		ask_user: createAskUserTool(options?.ask_user),
		todo_write: createTodoWriteTool(options?.todo_write),
		recall: createRecallTool(options?.recall),
		remember: createRememberTool(options?.remember),
	};
}
