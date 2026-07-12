import os from "node:os";
import { execSync } from "node:child_process";

export function getEnvironmentalContext(): Record<string, string> {
	let batteryLevel = "unknown";
	let activeApp = "terminal";

	try {
		if (os.platform() === "win32") {
			const output = execSync("wmic path Win32_Battery get EstimatedChargeRemaining", { encoding: "utf8" });
			const match = output.match(/\d+/);
			if (match) batteryLevel = match[0];
		} else if (os.platform() === "darwin") {
			const output = execSync("pmset -g batt", { encoding: "utf8" });
			const match = output.match(/(\d+)%/);
			if (match) batteryLevel = match[1];
		} else if (os.platform() === "linux") {
			const output = execSync("cat /sys/class/power_supply/BAT0/capacity", { encoding: "utf8" });
			if (output) batteryLevel = output.trim();
		}
	} catch (e) {
		// Ignore errors if battery info is unavailable
	}

	return {
		os_name: `${os.type()} ${os.release()}`,
		battery_level: batteryLevel,
		active_window_title: activeApp,
	};
}
