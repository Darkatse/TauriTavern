export function resolveSyncJobEventAction(payload) {
    const status = payload?.status;
    const origin = syncJobOrigin(payload);

    if (status === 'progress') {
        if (origin === 'scheduled') {
            return { type: 'ignore' };
        }
        return {
            type: 'progress',
            title: syncJobProgressTitle(payload),
            payload: payload.progress,
        };
    }

    if ((status === 'completed' || status === 'failed') && origin === 'remote_request') {
        return {
            type: 'report',
            report: {
                job: payload?.job,
                result: payload?.result,
            },
        };
    }

    return { type: 'ignore' };
}

export function syncFailureRequiresReload(result) {
    return result?.failure_kind === 'after_partial_local_mutation'
        || Boolean(result?.reconcile_error);
}

function syncJobOrigin(payload) {
    return payload?.job?.origin?.type || null;
}

function syncJobProgressTitle(payload) {
    return payload?.job?.endpoint?.type === 'remote_server'
        ? 'TT-Sync progress'
        : 'LAN Sync progress';
}
