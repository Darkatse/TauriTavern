import { execFileSync } from 'node:child_process';

const SEARCH_ROOTS = [
    'src',
    'scripts',
    'src-tauri',
    '.cache',
];

const RG_EXCLUDES = [
    '!node_modules/**',
    '!src-tauri/target/**',
    '!release/**',
    '!dist/**',
    '!.git/**',
];

const QUERIES = [
    {
        title: 'Phase 1: Anchor Zone (SPresets_script)',
        patterns: [
            '#openai_preset_import_file',
            '#completion_prompt_manager',
            '#extensions-settings-button .drawer-toggle',
        ],
    },
    {
        title: 'Phase 1: API Connections Gates',
        patterns: [
            "#main_api').on('change",
            "trigger('change')",
            'toggleChatCompletionForms',
            'showTypeSpecificControls',
            '[data-source]',
            '[data-tg-type]',
        ],
    },
    {
        title: 'Phase 1: Extensions Gates',
        patterns: [
            'regex_container',
            'extension_container',
        ],
    },
];

function runRg(pattern) {
    const args = [
        '--no-ignore',
        '--hidden',
        '-n',
        ...RG_EXCLUDES.flatMap((g) => ['-g', g]),
        pattern,
        ...SEARCH_ROOTS,
    ];

    try {
        return execFileSync('rg', args, { stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8' }).trim();
    } catch (error) {
        const stderr = String(error?.stderr || '').trim();
        // rg exits with 1 on no matches; treat as empty.
        if (String(error?.status) === '1') {
            return '';
        }
        throw new Error(`rg failed for pattern '${pattern}': ${stderr || error}`);
    }
}

function truncateLines(text, maxLines = 40) {
    const lines = String(text || '').split(/\r?\n/).filter(Boolean);
    if (lines.length <= maxLines) {
        return { total: lines.length, text: lines.join('\n') };
    }
    const shown = lines.slice(0, maxLines).join('\n');
    return { total: lines.length, text: `${shown}\n... (${lines.length - maxLines} more lines)` };
}

for (const query of QUERIES) {
    console.log(`\n## ${query.title}`);
    for (const pattern of query.patterns) {
        const output = runRg(pattern);
        if (!output) {
            console.log(`\n- ${pattern}: (no matches)`);
            continue;
        }
        const truncated = truncateLines(output);
        console.log(`\n- ${pattern} (${truncated.total} lines):\n${truncated.text}`);
    }
}
