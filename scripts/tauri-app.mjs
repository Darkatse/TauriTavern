#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const tauriCli = path.join(repoRoot, "node_modules", "@tauri-apps", "cli", "tauri.js");

if (!existsSync(tauriCli)) {
    console.error(`Missing Tauri CLI at ${tauriCli}. Run pnpm install first.`);
    process.exit(1);
}

const env = {
    ...process.env,
    TAURI_APP_PATH: path.join(repoRoot, "src-tauri", "crates", "tauritavern"),
    TAURI_FRONTEND_PATH: repoRoot,
};

const args = process.argv.slice(2);
if (args[0] === "--") {
    args.shift();
}

const result = spawnSync(process.execPath, [tauriCli, ...args], {
    cwd: repoRoot,
    stdio: "inherit",
    env,
});

if (result.error) {
    console.error(result.error.message);
    process.exit(1);
}

process.exit(result.status ?? 0);
