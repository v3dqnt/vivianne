import { type Component, Container, type TUI } from "@v3dqnt/vivianne-agent-tui";
import { visibleWidth } from "@v3dqnt/vivianne-agent-tui";

export class CenteredContainer extends Container {
	private getIsCentered: () => boolean;
	private maxWidth: number;

	constructor(getIsCentered: () => boolean, maxWidth: number = 100) {
		super();
		this.getIsCentered = getIsCentered;
		this.maxWidth = maxWidth;
	}

	render(width: number): string[] {
		const isCentered = this.getIsCentered();
		const targetWidth = isCentered ? Math.min(width, this.maxWidth) : width;
		const lines = super.render(targetWidth);
		
		if (!isCentered) return lines;
		
		const paddedLines: string[] = [];
		for (const line of lines) {
			const lw = visibleWidth(line);
			if (lw < width) {
				const padLeft = Math.floor((width - lw) / 2);
				paddedLines.push(' '.repeat(Math.max(0, padLeft)) + line);
			} else {
				paddedLines.push(line);
			}
		}
		return paddedLines;
	}
}

export class DynamicSpacer implements Component {
	private tui: TUI;
	private getIsCentered: () => boolean;

	constructor(tui: TUI, getIsCentered: () => boolean) {
		this.tui = tui;
		this.getIsCentered = getIsCentered;
	}
	
	invalidate(): void {}
	
	render(width: number): string[] {
		if (!this.getIsCentered()) return [];
		const rows = this.tui.terminal.rows;
		if (rows < 25) return [];
		const pad = Math.floor((rows - 20) / 2);
		return new Array(Math.max(0, pad)).fill("");
	}
}
