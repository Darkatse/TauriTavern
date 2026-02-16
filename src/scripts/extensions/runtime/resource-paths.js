export function normalizeExtensionResourcePath(resourcePath) {
    return String(resourcePath || '').replace(/^\/+/, '');
}

export function getExtensionResourceUrl(name, resourcePath) {
    const normalizedPath = normalizeExtensionResourcePath(resourcePath);
    return `/scripts/extensions/${name}/${normalizedPath}`;
}

export function isThirdPartyExtension(name) {
    return String(name || '').startsWith('third-party/');
}
