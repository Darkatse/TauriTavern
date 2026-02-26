import { getCurrentLocale } from '../../i18n.js';

export function normalizeExtensionResourcePath(resourcePath) {
    const locale = getCurrentLocale();
    return String(resourcePath || '')
        .replace(/^\/+/, '')
        .replace(/\$\{locale\}/g, locale);
}

export function getExtensionResourceUrl(name, resourcePath) {
    const normalizedPath = normalizeExtensionResourcePath(resourcePath);
    return `/scripts/extensions/${name}/${normalizedPath}`;
}

export function isThirdPartyExtension(name) {
    return String(name || '').startsWith('third-party/');
}
