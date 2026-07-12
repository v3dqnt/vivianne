import { fauxAssistantMessage, fauxToolCall } from "@v3dqnt/vivianne-agent-ai";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createHarness, getAssistantTexts, getUserTexts, type Harness } from "./harness.ts";

describe("todo_write stop gate", () => {
	const harnesses: Harness[] = [];

	afterEach(() => {
		vi.restoreAllMocks();
		while (harnesses.length > 0) {
			harnesses.pop()?.cleanup();
		}
	});

	it("nudges the agent to continue when it stops with unfinished todo items", async () => {
		const harness = await createHarness({ initialActiveToolNames: ["todo_write"] });
		harnesses.push(harness);

		harness.setResponses([
			fauxAssistantMessage(
				fauxToolCall("todo_write", {
					todos: [
						{ id: "1", content: "step one", status: "pending" },
						{ id: "2", content: "step two", status: "pending" },
					],
				}),
				{ stopReason: "toolUse" },
			),
			fauxAssistantMessage("I'll start with step one now."),
			fauxAssistantMessage(
				fauxToolCall("todo_write", {
					todos: [
						{ id: "1", content: "step one", status: "completed" },
						{ id: "2", content: "step two", status: "completed" },
					],
				}),
				{ stopReason: "toolUse" },
			),
			fauxAssistantMessage("All steps are done."),
		]);

		await harness.session.prompt("do the two-step task");

		expect(harness.faux.state.callCount).toBe(4);
		expect(getAssistantTexts(harness)).toContain("All steps are done.");
		expect(getUserTexts(harness).some((text) => text.includes("unfinished items"))).toBe(true);
	});

	it("does not nudge once the todo list is fully completed", async () => {
		const harness = await createHarness({ initialActiveToolNames: ["todo_write"] });
		harnesses.push(harness);

		harness.setResponses([
			fauxAssistantMessage(
				fauxToolCall("todo_write", { todos: [{ id: "1", content: "only step", status: "completed" }] }),
				{ stopReason: "toolUse" },
			),
			fauxAssistantMessage("Done."),
		]);

		await harness.session.prompt("do the one-step task");

		expect(harness.faux.state.callCount).toBe(2);
		expect(getUserTexts(harness).some((text) => text.includes("unfinished items"))).toBe(false);
	});

	it("caps auto-continuation at one nudge per user turn", async () => {
		const harness = await createHarness({ initialActiveToolNames: ["todo_write"] });
		harnesses.push(harness);

		harness.setResponses([
			fauxAssistantMessage(
				fauxToolCall("todo_write", { todos: [{ id: "1", content: "stubborn step", status: "pending" }] }),
				{ stopReason: "toolUse" },
			),
			fauxAssistantMessage("still not done"),
			fauxAssistantMessage("still not done again"),
		]);

		await harness.session.prompt("do a task the model won't finish");

		// tool_write call + first stop + one nudged continuation = 3 provider calls, then the
		// nudge budget is exhausted so the loop ends even though the todo item is still pending.
		expect(harness.faux.state.callCount).toBe(3);
		expect(getUserTexts(harness).filter((text) => text.includes("unfinished items"))).toHaveLength(1);
	});
});
