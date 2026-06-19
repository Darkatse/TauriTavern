export function normalizeWorldInfoActivationBatch(payload) {
    const entries = Array.from(payload?.activated?.entries?.values?.() ?? []).map(normalizeWorldInfoEntry);
    return {
        timestampMs: Date.now(),
        trigger: String(payload?.trigger || 'normal').trim() || 'normal',
        entries,
    };
}

function normalizeWorldInfoEntry(entry) {
    const position = normalizeWorldInfoPosition(entry?.position);
    return {
        world: typeof entry?.world === 'string' ? entry.world : '',
        uid: typeof entry?.uid === 'number' ? entry.uid : String(entry?.uid ?? '').trim(),
        displayName: normalizeWorldInfoDisplayName(entry),
        constant: Boolean(entry?.constant),
        content: String(entry?.content || ''),
        ...(position ? { position } : {}),
    };
}

function normalizeWorldInfoPosition(position) {
    switch (Number(position)) {
        case 0:
            return 'before';
        case 1:
            return 'after';
        case 2:
            return 'an_top';
        case 3:
            return 'an_bottom';
        case 4:
            return 'depth';
        case 5:
            return 'em_top';
        case 6:
            return 'em_bottom';
        case 7:
            return 'outlet';
        default:
            return undefined;
    }
}

function normalizeWorldInfoDisplayName(entry) {
    const comment = String(entry?.comment || '').trim();
    if (comment) {
        return comment;
    }

    if (Array.isArray(entry?.key)) {
        const key = entry.key.find((value) => String(value || '').trim());
        if (key !== undefined) {
            return String(key).trim();
        }
    }

    return String(entry?.uid ?? '').trim();
}
