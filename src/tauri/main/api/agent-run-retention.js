// @ts-check

const MAX_RUN_PRUNE_DETAIL_LIMIT = 1000;
const MAX_AGENT_RETENTION_KEEP_RUNS = 10000;

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
export function createAgentRunRetentionApi({ safeInvoke }) {
    async function readRetentionSettings() {
        const settings = await safeInvoke('get_tauritavern_settings');
        return normalizeRetentionSettings(settings?.agent?.retention);
    }

    async function updateRetentionSettings(input = {}) {
        const retention = normalizeRetentionSettingsPatch(input);
        if (Object.keys(retention).length === 0) {
            throw new Error('Agent retention update cannot be empty');
        }

        const settings = await safeInvoke('update_tauritavern_settings', {
            dto: {
                agent: {
                    retention: toSettingsRetentionPatch(retention),
                },
            },
        });
        return normalizeRetentionSettings(settings?.agent?.retention);
    }

    async function planRunPrune(input = {}) {
        if (!isPlainObject(input)) {
            throw new Error('Agent planRunPrune input must be an object');
        }

        const retention = input.retention == null
            ? undefined
            : normalizeRetentionSettings(input.retention);
        const detailLimit = normalizeRunPruneDetailLimit(input.detailLimit ?? input.detail_limit);

        return safeInvoke('plan_agent_run_prune', {
            dto: {
                ...(retention ? { retention: toRunPruneRetention(retention) } : {}),
                ...(detailLimit == null ? {} : { detailLimit }),
            },
        });
    }

    async function applyRunPrune(input = {}) {
        if (!isPlainObject(input)) {
            throw new Error('Agent applyRunPrune input must be an object');
        }

        const retention = input.retention == null
            ? undefined
            : normalizeRetentionSettings(input.retention);
        const detailLimit = normalizeRunPruneDetailLimit(input.detailLimit ?? input.detail_limit);

        return safeInvoke('apply_agent_run_prune', {
            dto: {
                ...(retention ? { retention: toRunPruneRetention(retention) } : {}),
                ...(detailLimit == null ? {} : { detailLimit }),
            },
        });
    }

    return {
        readSettings: readRetentionSettings,
        updateSettings: updateRetentionSettings,
        planPrune: planRunPrune,
        applyPrune: applyRunPrune,
    };
}

function normalizeRetentionSettings(value) {
    if (!isPlainObject(value)) {
        throw new Error('Agent retention settings must be an object');
    }

    const autoPruneEnabled = normalizeRetentionAutoPrune(
        value.autoPruneEnabled ?? value.auto_prune_enabled ?? false,
        'autoPruneEnabled',
    );
    const keepRecentTerminalRuns = normalizeRetentionRunCount(
        value.keepRecentTerminalRuns ?? value.keep_recent_terminal_runs,
        'keepRecentTerminalRuns',
    );
    const keepFullRecentRuns = normalizeRetentionRunCount(
        value.keepFullRecentRuns ?? value.keep_full_recent_runs,
        'keepFullRecentRuns',
    );

    if (keepFullRecentRuns > keepRecentTerminalRuns) {
        throw new Error('keepFullRecentRuns must be less than or equal to keepRecentTerminalRuns');
    }

    return {
        autoPruneEnabled,
        keepRecentTerminalRuns,
        keepFullRecentRuns,
    };
}

function normalizeRetentionSettingsPatch(value) {
    if (!isPlainObject(value)) {
        throw new Error('Agent retention settings update must be an object');
    }

    const patch = {};
    if (Object.prototype.hasOwnProperty.call(value, 'autoPruneEnabled')
        || Object.prototype.hasOwnProperty.call(value, 'auto_prune_enabled')) {
        patch.autoPruneEnabled = normalizeRetentionAutoPrune(
            value.autoPruneEnabled ?? value.auto_prune_enabled,
            'autoPruneEnabled',
        );
    }
    if (Object.prototype.hasOwnProperty.call(value, 'keepRecentTerminalRuns')
        || Object.prototype.hasOwnProperty.call(value, 'keep_recent_terminal_runs')) {
        patch.keepRecentTerminalRuns = normalizeRetentionRunCount(
            value.keepRecentTerminalRuns ?? value.keep_recent_terminal_runs,
            'keepRecentTerminalRuns',
        );
    }
    if (Object.prototype.hasOwnProperty.call(value, 'keepFullRecentRuns')
        || Object.prototype.hasOwnProperty.call(value, 'keep_full_recent_runs')) {
        patch.keepFullRecentRuns = normalizeRetentionRunCount(
            value.keepFullRecentRuns ?? value.keep_full_recent_runs,
            'keepFullRecentRuns',
        );
    }
    if (patch.keepRecentTerminalRuns != null
        && patch.keepFullRecentRuns != null
        && patch.keepFullRecentRuns > patch.keepRecentTerminalRuns) {
        throw new Error('keepFullRecentRuns must be less than or equal to keepRecentTerminalRuns');
    }
    return patch;
}

function normalizeRetentionAutoPrune(value, label) {
    if (typeof value !== 'boolean') {
        throw new Error(`${label} must be a boolean`);
    }
    return value;
}

function normalizeRetentionRunCount(value, label) {
    if (value == null || value === '') {
        throw new Error(`${label} is required`);
    }
    const count = Number(value);
    if (!Number.isInteger(count) || count < 0 || count > MAX_AGENT_RETENTION_KEEP_RUNS) {
        throw new Error(`${label} must be an integer between 0 and ${MAX_AGENT_RETENTION_KEEP_RUNS}`);
    }
    return count;
}

function normalizeRunPruneDetailLimit(value) {
    if (value == null) {
        return undefined;
    }
    const limit = Number(value);
    if (!Number.isInteger(limit) || limit < 0 || limit > MAX_RUN_PRUNE_DETAIL_LIMIT) {
        throw new Error(`detailLimit must be an integer between 0 and ${MAX_RUN_PRUNE_DETAIL_LIMIT}`);
    }
    return limit;
}

function toSettingsRetentionPatch(retention) {
    return {
        ...(retention.autoPruneEnabled == null ? {} : {
            auto_prune_enabled: retention.autoPruneEnabled,
        }),
        ...(retention.keepRecentTerminalRuns == null ? {} : {
            keep_recent_terminal_runs: retention.keepRecentTerminalRuns,
        }),
        ...(retention.keepFullRecentRuns == null ? {} : {
            keep_full_recent_runs: retention.keepFullRecentRuns,
        }),
    };
}

function toRunPruneRetention(retention) {
    return {
        keepRecentTerminalRuns: retention.keepRecentTerminalRuns,
        keepFullRecentRuns: retention.keepFullRecentRuns,
    };
}

function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
