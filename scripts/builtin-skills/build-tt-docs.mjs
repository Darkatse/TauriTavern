// Generates the built-in `tt-docs` Skill from `docs/`. The output is committed;
// `--check` verifies it is current (run by tests/builtin-skills.test.mjs).
import { mkdir, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const DOCS_ROOT = path.join(REPO_ROOT, 'docs');
export const TT_DOCS_ROOT = path.join(REPO_ROOT, 'default/skills/tt-docs');

const GROUPS = [
    {
        title: 'Page API (`window.__TAURITAVERN__.api`)',
        documents: [
            ['API/README.md', 'Start here for the page API: how to wait for the host to be ready, and which module covers what.'],
            ['API/Chat.md', '`api.chat`: a handle on the current or another chat that reads its saved messages from disk (paging back from the end, finding the last matching message, searching text) and keeps per-chat data. It does not switch the visible chat; the open chat is in memory as `SillyTavern.getContext().chat`.'],
            ['API/WorldInfo.md', '`api.worldInfo`: which world info entries the last generation activated, subscribing to activations, and opening an entry in the editor.'],
            ['API/Dev.md', '`api.dev`: frontend logs, backend logs, LLM API request and response logs, and the debug bundle export.'],
            ['API/Macros.md', 'Macros through `SillyTavern.getContext().macros`: registering macros and evaluating text against the live context or a captured one.'],
            ['API/ChatSurface.md', 'ChatSurface: how chat messages are rendered and virtualized, the renderer participants that JS-Slash-Runner and LittleWhiteBox register, and its errors and events.'],
            ['API/Layout.md', '`api.layout`: the content viewport, system bar areas, and screen capture.'],
            ['API/Extension.md', '`api.extension.store`: the key-value and blob storage extensions use for their data.'],
            ['API/Migration.md', 'How an extension can use TauriTavern\'s optional chat APIs (finding the last message, search, storage, history) instead of scanning `getContext().chat`.'],
            ['API/LlmConnections.md', '`api.llmConnections`: saved model connections that Agent profiles and Model Targets refer to.'],
            ['API/Agent.md', 'The Agent API: runs, Sessions, run history, profiles, tool catalogs, and registering extension tools.'],
            ['API/Skill.md', '`api.skill`: listing, reading, importing and installing Skills, and Skill scopes.'],
            ['API/MCP.md', '`api.mcp`: MCP server registrations, discovery, test calls, and how Agent runs use MCP tools.'],
            ['API/Database.md', '`api.db`: the embedded database extensions use (nodes, graphs, text and vector search, TQL).'],
        ],
    },
    {
        title: 'How the frontend works',
        documents: [
            ['FrontendGuide.md', 'How the frontend is put together: boot sequence, directory layout, how `/api/*` requests reach the Rust backend, chat payload and DOM boundaries, ChatSurface ownership, theme scope, Chat Completion parameters, and how third-party extensions are adapted.'],
            ['FrontendHostContract.md', 'What TauriTavern guarantees to pages and extensions: readiness signals, global objects such as `window.__TAURITAVERN__` and lodash `_`, request routing, compatibility patches, and the smoke tests that pin them down.'],
        ],
    },
    {
        title: 'Features and platforms',
        documents: [
            ['CurrentState/ThirdPartyExtensions.md', 'Third-party extensions: install and update rules, local and global folders and which wins, supported compatibility, what is not supported (such as server plugins), and regression checks.'],
            ['CurrentState/EmbeddedRuntime.md', 'The embedded runtime that manages message iframes for JS-Slash-Runner and LittleWhiteBox: its modes, how it runs, and how to debug it.'],
            ['CurrentState/LoggingObservability.md', 'Where logs go, how errors reach the user as notifications, and the developer observability tools.'],
            ['CurrentState/UserEndpointAccess.md', 'Custom API endpoints (custom URL, reverse proxy): the one-time native approval dialog, where approvals are stored, and what cancelling does.'],
            ['CurrentState/Sync.md', 'LAN Sync and TT-Sync: what data syncs, which settings sections sync independently, discovery and pairing, and automatic sync.'],
            ['CurrentState/DataDirectorySelection.md', 'Choosing or moving the data directory: what is supported and what is not.'],
            ['CurrentState/UpdateChannels.md', 'Stable and Canary update channels, how updates are detected, and public TestFlight builds.'],
            ['CurrentState/iOSPolicy.md', 'Features restricted in iOS builds: policy profiles and capabilities, how they are applied, and their known limits.'],
            ['CurrentState/MobileStyleAdaptation.md', 'The mobile content viewport and window background on Android, iOS and HarmonyOS, and where styles differ from upstream.'],
            ['CurrentState/TTS.md', 'System text-to-speech on Android and the built-in System TTS provider.'],
            ['CurrentState/CharacterIdentityContract.md', 'How characters keep their identity across renames and imports, how a character\'s chats are found, and how a card\'s embedded world info is adopted.'],
            ['CurrentState/InAppAssistant.md', 'The in-app assistant itself: its profile and lifecycle, Sessions and history, interface, and the boundaries of its tools.'],
        ],
    },
];

function skillText() {
    const lines = [
        '---',
        'name: tt-docs',
        'description: TauriTavern\'s own documentation for this version — the page API (window.__TAURITAVERN__.api), frontend structure, extension support, chat rendering, logs, custom endpoints, sync, data directory, updates, iOS and mobile limits, and the in-app assistant. Open its index when another TauriTavern skill points here, or when you need the exact behavior of a TauriTavern API or feature.',
        'license: AGPL-3.0',
        '---',
        '',
        '# TauriTavern documentation',
        '',
        'These are TauriTavern\'s own documents, from the same source revision as this app. They are written in Chinese. The layout under `references/` mirrors the project\'s `docs/` folder, so links between the documents listed here resolve; links to any other document point to files that are not included.',
    ];
    for (const group of GROUPS) {
        lines.push('', `## ${group.title}`, '');
        for (const [file, use] of group.documents) {
            lines.push(`- \`references/${file}\` — ${use}`);
        }
    }
    return `${lines.join('\n')}\n`;
}

/** Returns every output file, keyed by its path relative to the Skill directory. */
export async function buildTtDocs() {
    const files = new Map([['SKILL.md', Buffer.from(skillText())]]);
    for (const group of GROUPS) {
        for (const [file] of group.documents) {
            files.set(`references/${file}`, await readFile(path.join(DOCS_ROOT, file)));
        }
    }
    return files;
}

async function readTree(root, prefix = '') {
    const files = new Map();
    let entries;
    try {
        entries = await readdir(path.join(root, prefix), { withFileTypes: true });
    } catch (error) {
        if (error?.code === 'ENOENT') return files;
        throw error;
    }
    for (const entry of entries) {
        const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
        if (entry.isDirectory()) {
            for (const [file, bytes] of await readTree(root, relative)) files.set(file, bytes);
        } else {
            files.set(relative, await readFile(path.join(root, relative)));
        }
    }
    return files;
}

/** Lists the paths that differ between the generated and the committed Skill. */
export async function staleTtDocsFiles() {
    const expected = await buildTtDocs();
    const actual = await readTree(TT_DOCS_ROOT);
    const stale = [];
    for (const [file, bytes] of expected) {
        if (!actual.get(file)?.equals(bytes)) stale.push(file);
    }
    for (const file of actual.keys()) {
        if (!expected.has(file)) stale.push(file);
    }
    return stale.sort();
}

async function write() {
    const files = await buildTtDocs();
    const staging = `${TT_DOCS_ROOT}.tmp-${process.pid}`;
    await rm(staging, { recursive: true, force: true });
    for (const [file, bytes] of files) {
        const target = path.join(staging, file);
        await mkdir(path.dirname(target), { recursive: true });
        await writeFile(target, bytes);
    }
    await rm(TT_DOCS_ROOT, { recursive: true, force: true });
    await rename(staging, TT_DOCS_ROOT);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    if (process.argv.includes('--check')) {
        const stale = await staleTtDocsFiles();
        if (stale.length > 0) {
            console.error(`default/skills/tt-docs is out of date (${stale.join(', ')}). Run: pnpm run skills:tt-docs`);
            process.exit(1);
        }
    } else {
        await write();
    }
}
