import { Container, type Focusable, getKeybindings, Input, Spacer, Text, type TUI } from "@v3dqnt/vivianne-agent-tui";
import { theme } from "../theme/theme.ts";
import { DynamicBorder } from "./dynamic-border.ts";
import { keyHint } from "./keybinding-hints.ts";

export class PromptDialogComponent extends Container implements Focusable {
	private contentContainer: Container;
	private input: Input;
	private tui: TUI;
	private abortController = new AbortController();
	private inputResolver?: (value: string) => void;
	private inputRejecter?: (error: Error) => void;

	private _focused = false;
	get focused(): boolean { return this._focused; }
	set focused(value: boolean) { this._focused = value; this.input.focused = value; }

	constructor(tui: TUI, title: string) {
		super();
		this.tui = tui;

		this.addChild(new DynamicBorder());
		this.addChild(new Text(theme.fg("accent", theme.bold(title)), 1, 0));

		this.contentContainer = new Container();
		this.addChild(this.contentContainer);

		this.input = new Input();
		this.input.onSubmit = () => {
			if (this.inputResolver) {
				const value = this.input.getValue();
				this.replaceInputWithSubmittedText(value);
				this.inputResolver(value);
				this.inputResolver = undefined;
				this.inputRejecter = undefined;
			}
		};
		this.input.onEscape = () => this.cancel();

		this.addChild(new DynamicBorder());
	}

	private replaceInputWithSubmittedText(value: string): void {
		this.contentContainer.children = this.contentContainer.children.map((child) =>
			child === this.input ? new Text(`> ${value}`, 0, 0) : child,
		);
	}

	private cancel(): void {
		this.abortController.abort();
		if (this.inputRejecter) {
			this.inputRejecter(new Error("Prompt cancelled"));
			this.inputResolver = undefined;
			this.inputRejecter = undefined;
		}
	}

	showPrompt(message: string, placeholder?: string, recommendations?: string[]): Promise<string> {
		this.contentContainer.addChild(new Spacer(1));
		this.contentContainer.addChild(new Text(theme.fg("text", message), 1, 0));
		if (recommendations && recommendations.length > 0) {
			this.contentContainer.addChild(new Text(theme.fg("dim", "Recommendations:"), 1, 0));
			for (const r of recommendations) {
				this.contentContainer.addChild(new Text(theme.fg("dim", ` - ${r}`), 1, 0));
			}
		}
		if (placeholder) {
			this.contentContainer.addChild(new Text(theme.fg("dim", `e.g., ${placeholder}`), 1, 0));
		}
		this.contentContainer.addChild(this.input);
		this.contentContainer.addChild(
			new Text(`(${keyHint("tui.select.cancel", "to cancel,")} ${keyHint("tui.select.confirm", "to submit")})`, 1, 0),
		);

		this.input.setValue("");
		this.tui.requestRender();

		return new Promise((resolve, reject) => {
			this.inputResolver = resolve;
			this.inputRejecter = reject;
		});
	}

	handleInput(data: string): void {
		const kb = getKeybindings();
		if (kb.matches(data, "tui.select.cancel")) {
			this.cancel();
			return;
		}
		this.input.handleInput(data);
	}
}
