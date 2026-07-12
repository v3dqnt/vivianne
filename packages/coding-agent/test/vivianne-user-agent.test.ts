import { describe, expect, it } from "vitest";
import { getVivianneUserAgent } from "../src/utils/vivianne-user-agent.ts";

describe("getVivianneUserAgent", () => {
	it("formats the user agent expected by vivianne.dev", () => {
		const runtime = process.versions.bun ? `bun/${process.versions.bun}` : `node/${process.version}`;
		const userAgent = getVivianneUserAgent("1.2.3");

		expect(userAgent).toBe(`pi/1.2.3 (${process.platform}; ${runtime}; ${process.arch})`);
		expect(userAgent).toMatch(/^pi\/[^\s()]+ \([^;()]+;\s*[^;()]+;\s*[^()]+\)$/);
	});
});
