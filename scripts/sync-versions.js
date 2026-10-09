#!/usr/bin/env node

/**
 * Syncs all workspace package dependency versions to match their current
 * versions, then propagates that version into the two non-npm files that
 * also carry it: the Tauri crate's `Cargo.toml` and `tauri.conf.json`.
 *
 * Those two are not optional extras. `tauri.conf.json`'s version is what
 * lands in the release's `latest.json` and what every installed client
 * compares against to decide whether an update exists; `Cargo.toml`'s is what
 * `env!("CARGO_PKG_VERSION")` returns, which drives `cmd_app_version` and the
 * in-app "what's new" dialog. Bumping only the package.json files (which is
 * all this script used to do) left both at the old number, so a release would
 * build, publish, and then be ignored by every client as "not newer" — a
 * failure with no error message anywhere.
 */

import { readFileSync, writeFileSync, readdirSync } from 'fs';
import { join } from 'path';

const packagesDir = join(process.cwd(), 'packages');
const packageDirs = readdirSync(packagesDir, { withFileTypes: true })
	.filter(dirent => dirent.isDirectory())
	.map(dirent => dirent.name);

// Read all package.json files and build version map
const packages = {};
const versionMap = {};

for (const dir of packageDirs) {
	const pkgPath = join(packagesDir, dir, 'package.json');
	try {
		const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));
		// Unversioned packages (scratch projects like a trailer render) aren't
		// released, so they sit outside the lockstep.
		if (!pkg.version) continue;
		packages[dir] = { path: pkgPath, data: pkg };
		versionMap[pkg.name] = pkg.version;
	} catch (e) {
		console.error(`Failed to read ${pkgPath}:`, e.message);
	}
}

console.log('Current versions:');
for (const [name, version] of Object.entries(versionMap).sort()) {
	console.log(`  ${name}: ${version}`);
}

// Verify all versions are the same (lockstep)
const versions = new Set(Object.values(versionMap));
if (versions.size > 1) {
	console.error('\n❌ ERROR: Not all packages have the same version!');
	console.error('Expected lockstep versioning. Run one of:');
	console.error('  npm run version:patch');
	console.error('  npm run version:minor');
	console.error('  npm run version:major');
	process.exit(1);
}

console.log('\n✅ All packages at same version (lockstep)');

// Update all inter-package dependencies
let totalUpdates = 0;
for (const [dir, pkg] of Object.entries(packages)) {
	let updated = false;
	
	// Check dependencies
	if (pkg.data.dependencies) {
		for (const [depName, currentVersion] of Object.entries(pkg.data.dependencies)) {
			if (versionMap[depName]) {
				const newVersion = `^${versionMap[depName]}`;
				if (currentVersion !== newVersion) {
					console.log(`\n${pkg.data.name}:`);
					console.log(`  ${depName}: ${currentVersion} → ${newVersion}`);
					pkg.data.dependencies[depName] = newVersion;
					updated = true;
					totalUpdates++;
				}
			}
		}
	}
	
	// Check devDependencies
	if (pkg.data.devDependencies) {
		for (const [depName, currentVersion] of Object.entries(pkg.data.devDependencies)) {
			if (versionMap[depName]) {
				const newVersion = `^${versionMap[depName]}`;
				if (currentVersion !== newVersion) {
					console.log(`\n${pkg.data.name}:`);
					console.log(`  ${depName}: ${currentVersion} → ${newVersion} (devDependencies)`);
					pkg.data.devDependencies[depName] = newVersion;
					updated = true;
					totalUpdates++;
				}
			}
		}
	}
	
	// Write if updated
	if (updated) {
		writeFileSync(pkg.path, JSON.stringify(pkg.data, null, '\t') + '\n');
	}
}

if (totalUpdates === 0) {
	console.log('\nAll inter-package dependencies already in sync.');
} else {
	console.log(`\n✅ Updated ${totalUpdates} dependency version(s)`);
}

// ── Non-npm version carriers ──────────────────────────────────────────────
// Written with targeted regex/JSON edits rather than a full re-serialize, so
// Cargo.toml keeps its comments and formatting.

const targetVersion = Object.values(versionMap)[0];

// packages/gui/src-tauri/Cargo.toml — only the [package] version, which is
// the first `version = "..."` in the file. A blind global replace would also
// rewrite every dependency's version pin.
const cargoPath = join(packagesDir, 'gui', 'src-tauri', 'Cargo.toml');
try {
	const cargo = readFileSync(cargoPath, 'utf8');
	const updated = cargo.replace(/^version = "[^"]*"/m, `version = "${targetVersion}"`);
	if (updated !== cargo) {
		writeFileSync(cargoPath, updated);
		console.log(`\n✅ Cargo.toml → ${targetVersion}`);
	} else {
		console.log(`\nCargo.toml already at ${targetVersion}`);
	}
} catch (e) {
	console.error(`\n❌ Failed to update ${cargoPath}:`, e.message);
	process.exit(1);
}

// packages/gui/src-tauri/tauri.conf.json
const confPath = join(packagesDir, 'gui', 'src-tauri', 'tauri.conf.json');
try {
	const conf = JSON.parse(readFileSync(confPath, 'utf8'));
	if (conf.version !== targetVersion) {
		conf.version = targetVersion;
		writeFileSync(confPath, JSON.stringify(conf, null, 2) + '\n');
		console.log(`✅ tauri.conf.json → ${targetVersion}`);
	} else {
		console.log(`tauri.conf.json already at ${targetVersion}`);
	}
} catch (e) {
	console.error(`❌ Failed to update ${confPath}:`, e.message);
	process.exit(1);
}
