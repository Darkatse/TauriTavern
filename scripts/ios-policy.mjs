#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const ALLOWED_PROFILES = new Set(['full', 'ios_internal_full', 'ios_external_beta']);

function printHelp() {
    console.log(`Usage: node scripts/ios-policy.mjs <dev|build> [options] [-- <tauri args>]

Options:
  --profile <profile>     iOS policy profile (full | ios_internal_full | ios_external_beta)
  --help                  Show this help message

Behavior:
  - Sets TAURITAVERN_IOS_POLICY_PROFILE for the build/dev process.
  - For profile ios_internal_full / ios_external_beta, iOS builds add:
      --export-method app-store-connect
    (full does not specify export-method).
`);
}

function resolveRepoRoot() {
    const scriptDir = path.dirname(fileURLToPath(import.meta.url));
    return path.resolve(scriptDir, '..');
}

function parseArgs(argv) {
    const options = {
        mode: null,
        profile: null,
        extraTauriArgs: [],
    };

    if (argv.length === 0 || argv[0] === '--help' || argv[0] === '-h') {
        printHelp();
        process.exit(0);
    }

    options.mode = argv[0];

    let passThrough = false;
    for (let index = 1; index < argv.length; index += 1) {
        const value = argv[index];

        if (passThrough) {
            options.extraTauriArgs.push(value);
            continue;
        }

        if (value === '--') {
            const next = argv[index + 1];
            if (next === '--profile' || next === '--help' || next === '-h') {
                continue;
            }

            passThrough = true;
            options.extraTauriArgs.push('--');
            continue;
        }

        if (value === '--help' || value === '-h') {
            printHelp();
            process.exit(0);
        }

        if (value === '--profile') {
            const profile = argv[index + 1];
            if (!profile) {
                throw new Error('Missing value for --profile');
            }
            options.profile = profile;
            index += 1;
            continue;
        }

        options.extraTauriArgs.push(value);
    }

    return options;
}

function shouldAddExportMethod(profile) {
    return profile === 'ios_internal_full' || profile === 'ios_external_beta';
}

function hasExportMethodFlag(args) {
    return args.some((value) => value === '--export-method' || value.startsWith('--export-method='));
}

function main() {
    const repoRoot = resolveRepoRoot();
    process.chdir(repoRoot);

    const options = parseArgs(process.argv.slice(2));
    if (options.mode !== 'dev' && options.mode !== 'build') {
        throw new Error(`Unsupported mode: ${options.mode}. Expected 'dev' or 'build'.`);
    }

    const profile = (options.profile || '').trim();
    if (!ALLOWED_PROFILES.has(profile)) {
        throw new Error(
            `Invalid --profile value: ${profile || '(missing)'}. Expected one of: full, ios_internal_full, ios_external_beta.`,
        );
    }

    if (process.platform !== 'darwin') {
        throw new Error('iOS dev/build requires macOS.');
    }

    const tauriCli = path.join(repoRoot, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
    if (!existsSync(tauriCli)) {
        throw new Error(`Missing Tauri CLI at ${tauriCli}. Run pnpm install first.`);
    }

    const env = {
        ...process.env,
        TAURITAVERN_IOS_POLICY_PROFILE: profile,
    };

    const tauriArgs = ['ios', options.mode, ...options.extraTauriArgs];

    if (
        options.mode === 'build'
        && shouldAddExportMethod(profile)
        && !hasExportMethodFlag(tauriArgs)
    ) {
        tauriArgs.splice(2, 0, '--export-method', 'app-store-connect');
    }

    const result = spawnSync(process.execPath, [tauriCli, ...tauriArgs], {
        stdio: 'inherit',
        env,
    });

    if (result.error) {
        throw result.error;
    }
    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }
}

try {
    main();
} catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
}
