import * as fs from "node:fs";
import { Container, Image, Spacer, Text } from "@v3dqnt/vivianne-agent-tui";
import { getBundledInteractiveAssetPath } from "../../../config.ts";
import { theme } from "../theme/theme.ts";
import { DynamicBorder } from "./dynamic-border.ts";

const BLOG_URL = "https://mariozechner.at/posts/2026-04-08-ive-sold-out/";
const IMAGE_FILENAME = "clankolas.png";

let cachedImageBase64: string | undefined;
let attemptedImageLoad = false;

function loadImageBase64(): string | undefined {
	if (attemptedImageLoad) {
		return cachedImageBase64;
	}

	attemptedImageLoad = true;
	try {
		cachedImageBase64 = fs.readFileSync(getBundledInteractiveAssetPath(IMAGE_FILENAME)).toString("base64");
	} catch {
		cachedImageBase64 = undefined;
	}
	return cachedImageBase64;
}

export class V3dqntAnnouncementComponent extends Container {
	constructor() {
		super();
	}
}
