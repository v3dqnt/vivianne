import { Type, type Static } from "typebox";
import type { AgentTool } from "@v3dqnt/vivianne-agent-agent-core";
import { Text } from "@v3dqnt/vivianne-agent-tui";
import { keyboard, mouse, Point, Button } from "@nut-tree-fork/nut-js";
import { wrapToolDefinition } from "./tool-definition-wrapper.ts";
import type { ToolDefinition, ToolRenderResultOptions } from "../extensions/types.ts";
import * as os from "node:os";
import * as path from "node:path";
import * as fs from "node:fs/promises";
import { exec } from "node:child_process";
import { promisify } from "node:util";
import type { Theme } from "../../modes/interactive/theme/theme.ts";
import { processImage } from "../../utils/image-process.ts";

const execAsync = promisify(exec);

// Configure nut.js
keyboard.config.autoDelayMs = 25;
mouse.config.autoDelayMs = 25;

const computerSchema = Type.Object({
	action: Type.Union(
		[
			Type.Literal("key"),
			Type.Literal("type"),
			Type.Literal("mouse_move"),
			Type.Literal("left_click"),
			Type.Literal("left_click_drag"),
			Type.Literal("right_click"),
			Type.Literal("middle_click"),
			Type.Literal("double_click"),
			Type.Literal("screenshot"),
			Type.Literal("cursor_position"),
		],
		{ description: "The action to perform." },
	),
	coordinate: Type.Optional(
		Type.Array(Type.Number(), { minItems: 2, maxItems: 2, description: "X,Y coordinates for mouse actions." }),
	),
	text: Type.Optional(Type.String({ description: "Text to type or keys to press." })),
});

export type ComputerToolInput = Static<typeof computerSchema>;

export interface ComputerToolOptions {
	autoResizeImages?: boolean;
}

export function createComputerToolDefinition(options?: ComputerToolOptions): ToolDefinition<typeof computerSchema, undefined> {
	const autoResizeImages = options?.autoResizeImages ?? true;

	return {
		name: "computer",
		label: "computer",
		description: "Control the computer: use mouse, keyboard, and take screenshots.",
		promptSnippet: "Use the computer tool to control the UI",
		promptGuidelines: [
			"Always check cursor_position or take a screenshot to orient yourself before clicking.",
			"For key combinations, use standard names (e.g., 'Control+C' or 'Return').",
		],
		parameters: computerSchema,
		async execute(_id, args, signal) {
			if (signal?.aborted) throw new Error("Operation aborted");
			let outputText = "";

			switch (args.action) {
				case "mouse_move":
					if (args.coordinate) {
						await mouse.setPosition(new Point(args.coordinate[0], args.coordinate[1]));
						outputText = `Moved mouse to X=${args.coordinate[0]}, Y=${args.coordinate[1]}`;
					} else {
						throw new Error("coordinate is required for mouse_move");
					}
					break;
				case "left_click":
					await mouse.leftClick();
					outputText = "Left clicked";
					break;
				case "right_click":
					await mouse.rightClick();
					outputText = "Right clicked";
					break;
				case "middle_click":
					// Button 1 is usually Middle
					await mouse.click(1 as Button);
					outputText = "Middle clicked";
					break;
				case "double_click":
					await mouse.leftClick();
					await mouse.leftClick();
					outputText = "Double clicked";
					break;
				case "left_click_drag":
					if (args.coordinate) {
						await mouse.pressButton(Button.LEFT);
						await mouse.setPosition(new Point(args.coordinate[0], args.coordinate[1]));
						await mouse.releaseButton(Button.LEFT);
						outputText = `Dragged to X=${args.coordinate[0]}, Y=${args.coordinate[1]}`;
					} else {
						throw new Error("coordinate is required for left_click_drag");
					}
					break;
				case "type":
					if (args.text) {
						await keyboard.type(args.text);
						outputText = `Typed: ${args.text}`;
					} else {
						throw new Error("text is required for type");
					}
					break;
				case "key":
					if (args.text) {
						// Simple mapping logic: in a real implementation we would map strings to Key enum
						// For now, if length is 1, type it.
						await keyboard.type(args.text);
						outputText = `Pressed key: ${args.text}`;
					} else {
						throw new Error("text is required for key");
					}
					break;
				case "cursor_position":
					const pos = await mouse.getPosition();
					outputText = `Cursor is at X=${pos.x}, Y=${pos.y}`;
					break;
				case "screenshot":
					// Fallback to powershell for reliable Windows screenshots
					const tempDir = os.tmpdir();
					const filename = `screenshot-${Date.now()}.png`;
					const capturePath = path.join(tempDir, filename);

					if (os.platform() === "win32") {
						const psCommand = `Add-Type -AssemblyName System.Drawing; $b = New-Object System.Drawing.Bitmap 1920, 1080; $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen(0,0,0,0, $b.Size); $b.Save('${capturePath}')`;
						await execAsync(`powershell -c "${psCommand}"`);
					} else if (os.platform() === "darwin") {
						await execAsync(`screencapture -x "${capturePath}"`);
					} else {
						throw new Error("Screenshot not implemented for this OS in the computer tool fallback");
					}

					try {
						const buffer = await fs.readFile(capturePath);
						const processed = await processImage(buffer, "image/png", { autoResizeImages });
						
						// Cleanup temp file
						await fs.unlink(capturePath).catch(() => {});

						if (!processed.ok) {
							return { content: [{ type: "text", text: `Screenshot captured but failed to process: ${processed.message}` }], details: undefined };
						}
						
						let textNote = `Screenshot captured [${processed.mimeType}]`;
						if (processed.hints.length > 0) textNote += `\n${processed.hints.join("\n")}`;

						return {
							content: [
								{ type: "text", text: textNote },
								{ type: "image", data: processed.data, mimeType: processed.mimeType as any }
							],
							details: undefined
						};
					} catch (e: any) {
						throw new Error(`Failed to capture/read screenshot: ${e.message}`);
					}
			}

			return { content: [{ type: "text", text: outputText }], details: undefined };
		},
		renderCall(args, theme, ctx) {
			const text = (ctx.lastComponent as Text | undefined) ?? new Text("", 0, 0);
			let detail = args.action;
			if (args.coordinate) detail += ` [${args.coordinate[0]}, ${args.coordinate[1]}]`;
			if (args.text) detail += ` "${args.text}"`;
			text.setText(theme.fg("toolTitle", theme.bold("computer")) + " " + theme.fg("accent", detail));
			return text;
		},
		renderResult(result, options: ToolRenderResultOptions, theme: Theme, ctx) {
			const text = (ctx.lastComponent as Text | undefined) ?? new Text("", 0, 0);
			if (options.expanded) {
				const contentStr = result.content
					.filter((c) => c.type === "text")
					.map((c) => c.text)
					.join("\n");
				text.setText(theme.fg("toolOutput", contentStr || "Computer action completed"));
			} else if (ctx.isError) {
                text.setText("");
            }
			return text;
		},
	};
}

export function createComputerTool(options?: ComputerToolOptions): AgentTool<typeof computerSchema> {
	return wrapToolDefinition(createComputerToolDefinition(options));
}
