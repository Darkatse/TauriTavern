#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const PRODUCT_NAME = "TauriTavern";
const BINARY_CANDIDATES = ["tauritavern", "TauriTavern"];

function printHelp() {
    console.log(`Usage: node scripts/build-portable.mjs [options] [-- <extra tauri args>]

Options:
  --target <triple>       Rust target triple, e.g. x86_64-pc-windows-msvc
  --output-dir <path>     Portable output directory (default: release/portable)
  --skip-web-build        Skip frontend webpack build
  --help                  Show this help message
`);
}

function quoteShellArgument(value) {
    if (value.length === 0) {
        return "\"\"";
    }
    if (!/[\s"]/u.test(value)) {
        return value;
    }
    return `"${value.replace(/"/gu, '\\"')}"`;
}

function run(command, args, cwd) {
    const commandLine = [command, ...args.map(quoteShellArgument)].join(" ");
    const result = spawnSync(commandLine, { cwd, stdio: "inherit", shell: true });
    if (result.error) {
        throw result.error;
    }
    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }
}

function parseArgs(argv) {
    const options = {
        target: null,
        outputDir: "release/portable",
        skipWebBuild: false,
        extraTauriArgs: [],
    };

    let passThrough = false;
    for (let index = 0; index < argv.length; index += 1) {
        const value = argv[index];

        if (passThrough) {
            options.extraTauriArgs.push(value);
            continue;
        }

        if (value === "--") {
            passThrough = true;
            continue;
        }

        if (value === "--help" || value === "-h") {
            printHelp();
            process.exit(0);
        }

        if (value === "--skip-web-build") {
            options.skipWebBuild = true;
            continue;
        }

        if (value === "--target") {
            const target = argv[index + 1];
            if (!target) {
                throw new Error("Missing value for --target");
            }
            options.target = target;
            index += 1;
            continue;
        }

        if (value === "--output-dir") {
            const outputDir = argv[index + 1];
            if (!outputDir) {
                throw new Error("Missing value for --output-dir");
            }
            options.outputDir = outputDir;
            index += 1;
            continue;
        }

        options.extraTauriArgs.push(value);
    }

    return options;
}

function resolveReleaseDirectory(repoRoot, target) {
    const releaseRoot = target
        ? path.join(repoRoot, "src-tauri", "target", target, "release")
        : path.join(repoRoot, "src-tauri", "target", "release");
    return releaseRoot;
}

function resolvePortableBinary(releaseDirectory) {
    const executableSuffix = process.platform === "win32" ? ".exe" : "";

    for (const candidate of BINARY_CANDIDATES) {
        const binaryPath = path.join(releaseDirectory, `${candidate}${executableSuffix}`);
        if (existsSync(binaryPath) && statSync(binaryPath).isFile()) {
            return binaryPath;
        }
    }

    const fallback = readdirSync(releaseDirectory, { withFileTypes: true })
        .filter((entry) => entry.isFile())
        .map((entry) => entry.name)
        .find((name) => {
            if (process.platform === "win32") {
                return name.toLowerCase().endsWith(".exe");
            }
            return !name.includes(".");
        });

    if (fallback) {
        return path.join(releaseDirectory, fallback);
    }

    throw new Error(`Unable to locate built portable binary in ${releaseDirectory}`);
}

function normalizePlatform(platform) {
    if (platform === "win32") {
        return "windows";
    }
    if (platform === "darwin") {
        return "macos";
    }
    return platform;
}

function normalizeArch(arch) {
    if (arch === "x64") {
        return "x64";
    }
    if (arch === "ia32") {
        return "x86";
    }
    return arch;
}

function buildPlatformTag(target) {
    if (target) {
        return target.replace(/[^a-zA-Z0-9._-]/g, "_");
    }
    return `${normalizePlatform(process.platform)}-${normalizeArch(process.arch)}`;
}

function main() {
    const repoRoot = process.cwd();
    const options = parseArgs(process.argv.slice(2));

    if (!options.skipWebBuild) {
        run("pnpm", ["run", "web:build"], repoRoot);
    }

    const tauriArgs = ["exec", "tauri", "build", "--no-bundle", "--features", "portable"];
    if (options.target) {
        tauriArgs.push("--target", options.target);
    }
    if (options.extraTauriArgs.length > 0) {
        tauriArgs.push(...options.extraTauriArgs);
    }
    run("pnpm", tauriArgs, repoRoot);

    const releaseDirectory = resolveReleaseDirectory(repoRoot, options.target);
    const binaryPath = resolvePortableBinary(releaseDirectory);
    const outputDirectory = path.resolve(repoRoot, options.outputDir);
    mkdirSync(outputDirectory, { recursive: true });

    const extension = process.platform === "win32" ? ".exe" : "";
    const artifactName = `${PRODUCT_NAME}_${buildPlatformTag(options.target)}_portable${extension}`;
    const artifactPath = path.join(outputDirectory, artifactName);
    copyFileSync(binaryPath, artifactPath);

    console.log(`Portable binary copied to ${artifactPath}`);
}

try {
    main();
} catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
}
