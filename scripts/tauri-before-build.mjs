#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

if (process.env.TAURITAVERN_SKIP_WEB_BUILD === "1") {
    console.log("Skipping frontend bundle build by request.");
    process.exit(0);
}

const result = spawnSync("pnpm run web:build", {
    cwd: repoRoot,
    stdio: "inherit",
    shell: true,
});

if (result.error) {
    console.error(result.error.message);
    process.exit(1);
}

process.exit(result.status ?? 1);
