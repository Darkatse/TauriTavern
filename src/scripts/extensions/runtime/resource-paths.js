import { getCurrentLocale } from '../../i18n.js';

export function normalizeExtensionResourcePath(resourcePath) {
    let path = String(resourcePath || '').replace(/^\/+/, '');
    
    // 如果路径包含 ${locale} 占位符，尝试替换为当前语言 ID
    if (path.includes('${locale}')) {
        try {
            const locale = getCurrentLocale();
            path = path.replace(/\$\{locale\}/g, locale);
        } catch (e) {
            console.warn('解析扩展资源路径中的 ${locale} 失败:', e);
        }
    }
    
    return path;
}

export function getExtensionResourceUrl(name, resourcePath) {
    const normalizedPath = normalizeExtensionResourcePath(resourcePath);
    return `/scripts/extensions/${name}/${normalizedPath}`;
}

export function isThirdPartyExtension(name) {
    return String(name || '').startsWith('third-party/');
}
