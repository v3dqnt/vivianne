import { describe, expect, it } from "vitest";
import { createTodoWriteTool, TodoListStore } from "../src/core/tools/todo.ts";

function getTextOutput(result: any): string {
	return (
		result.content
			?.filter((c: any) => c.type === "text")
			.map((c: any) => c.text)
			.join("\n") || ""
	);
}

describe("todo_write tool", () => {
	it("replaces the store's item list and reports progress", async () => {
		const store = new TodoListStore();
		const tool = createTodoWriteTool({ store });

		const result = await tool.execute("call-1", {
			todos: [
				{ id: "1", content: "first step", status: "pending" },
				{ id: "2", content: "second step", status: "completed" },
			],
		});

		expect(store.getItems()).toHaveLength(2);
		expect(getTextOutput(result)).toContain("1/2 completed");
	});

	it("tracks incomplete items via hasIncomplete", async () => {
		const store = new TodoListStore();
		const tool = createTodoWriteTool({ store });

		await tool.execute("call-1", { todos: [{ id: "1", content: "step", status: "pending" }] });
		expect(store.hasIncomplete()).toBe(true);

		await tool.execute("call-2", { todos: [{ id: "1", content: "step", status: "completed" }] });
		expect(store.hasIncomplete()).toBe(false);
	});

	it("invokes onChange when the list is replaced", async () => {
		const store = new TodoListStore();
		const seen: number[] = [];
		store.onChange = (items) => seen.push(items.length);
		const tool = createTodoWriteTool({ store });

		await tool.execute("call-1", { todos: [{ id: "1", content: "step", status: "pending" }] });
		await tool.execute("call-2", { todos: [] });

		expect(seen).toEqual([1, 0]);
	});

	it("defaults to a fresh internal store when none is provided", async () => {
		const tool = createTodoWriteTool();
		const result = await tool.execute("call-1", { todos: [{ id: "1", content: "step", status: "pending" }] });
		expect(getTextOutput(result)).toContain("0/1 completed");
	});
});
