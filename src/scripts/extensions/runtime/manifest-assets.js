export function resolveExtensionAssetPath(assetField) {
    const candidate = Array.isArray(assetField) && assetField.length === 1
        ? assetField[0]
        : assetField;

    if (typeof candidate !== 'string') {
        return null;
    }

    if (!candidate.trim()) {
        return null;
    }

    if (Array.isArray(assetField) && assetField.length !== 1) {
        return null;
    }

    return candidate;
}
