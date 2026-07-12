import type { AgentTool } from "@v3dqnt/vivianne-agent-agent-core";
import { Text } from "@v3dqnt/vivianne-agent-tui";
import { type Static, Type } from "typebox";
import type { Theme } from "../../modes/interactive/theme/theme.ts";
import type { ToolDefinition } from "../extensions/types.ts";
import { wrapToolDefinition } from "./tool-definition-wrapper.ts";

const todoItemSchema = Type.Object({
	id: Type.String({ description: "Short stable identifier for this item, e.g. '1', '2'." }),
	content: Type.String({ description: "The step to do, phrased as an imperative action." }),
	status: Type.Union([Type.Literal("pending"), Type.Literal("in_progress"), Type.Literal("completed")], {
		description: "Current status of this step.",
	}),
});

const todoWriteSchema = Type.Object({
	todos: Type.Array(todoItemSchema, {
		description: "The full current list of todo items. This replaces the previous list entirely.",
	}),
});

export type TodoItem = Static<typeof todoItemSchema>;
export type TodoWriteToolInput = Static<typeof todoWriteSchema>;

/** Shared, mutable store for the current task's todo list. AgentSession owns one instance per session. */
export class TodoListStore {
	private _items: TodoItem[] = [];
	/** Invoked whenever the list is replaced, e.g. so the UI/session can refresh. */
	onChange?: (items: TodoItem[]) => void;

	getItems(): TodoItem[] {
		return this._items;
	}

	setItems(items: TodoItem[]): void {
		this._items = items;
		this.onChange?.(items);
	}

	hasIncomplete(): boolean {
		return this._items.some((item) => item.status !== "completed");
	}
}

export interface TodoWriteToolOptions {
	/** Defaults to a fresh, session-local store when omitted. */
	store?: TodoListStore;
}

const STATUS_GLYPH: Record<TodoItem["status"], string> = {
	pending: "☐",
	in_progress: "▶",
	completed: "☑",
};

function formatTodoList(todos: TodoItem[], theme: Theme): string {
	if (todos.length === 0) {
		return theme.fg("muted", "(empty todo list)");
	}
	return todos
		.map((item) => {
			const glyph = STATUS_GLYPH[item.status];
			const text = item.status === "completed" ? theme.strikethrough(item.content) : item.content;
			const color = item.status === "completed" ? "muted" : item.status === "in_progress" ? "accent" : "toolOutput";
			return theme.fg(color, `${glyph} ${text}`);
		})
		.join("\n");
}

export function createTodoWriteToolDefinition(
	options?: TodoWriteToolOptions,
): ToolDefinition<typeof todoWriteSchema, TodoItem[]> {
	const store = options?.store ?? new TodoListStore();
	return {
		name: "todo_write",
		label: "todo_write",
		description:
			"Create or update the todo list for the current task. Use it for tasks with 3+ distinct steps you might otherwise lose track of - multi-file refactors, 'fix all the failing tests', anything where the user gave a list. Always pass the full list (this replaces the previous one): mark items done as you finish them, and add items you discover mid-task. Skip it for anything you'll finish in a handful of calls.",
		promptSnippet: "Track and update a todo list for multi-step tasks",
		parameters: todoWriteSchema,
		async execute(_toolCallId, { todos }: TodoWriteToolInput, _signal, _onUpdate, _ctx) {
			store.setItems(todos);
			const pending = todos.filter((t) => t.status !== "completed").length;
			const summary =
				todos.length === 0
					? "Todo list cleared."
					: `Todo list updated (${todos.length - pending}/${todos.length} completed).`;
			return { content: [{ type: "text", text: summary }], details: todos };
		},
		renderCall(_args, theme) {
			const text = new Text("", 0, 0);
			text.setText(theme.fg("toolTitle", theme.bold("todo_write")));
			return text;
		},
		renderResult(result, _options, theme) {
			const text = new Text("", 0, 0);
			const todos = (result.details as TodoItem[] | undefined) ?? [];
			text.setText(`\n${formatTodoList(todos, theme)}`);
			return text;
		},
	};
}

export function createTodoWriteTool(options?: TodoWriteToolOptions): AgentTool<typeof todoWriteSchema> {
	return wrapToolDefinition(createTodoWriteToolDefinition(options));
}
