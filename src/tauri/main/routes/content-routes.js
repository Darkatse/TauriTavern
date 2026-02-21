import { normalizeBinaryPayload } from '../binary-utils.js';

const EXTERNAL_IMPORT_ALLOWED_URL_PATTERNS = [
    /^https:\/\/cdn\.discordapp\.com\/attachments\/.+/i,
    /^https:\/\/files\.catbox\.moe\/.+/i,
];

function parseUrl(value) {
    try {
        return new URL(String(value || '').trim());
    } catch {
        return null;
    }
}

function isWhitelistedExternalImportUrl(url) {
    const normalized = url.toString();
    return EXTERNAL_IMPORT_ALLOWED_URL_PATTERNS.some((pattern) => pattern.test(normalized));
}

function isPngResponse(url, contentType) {
    const loweredType = String(contentType || '').toLowerCase();
    if (loweredType.startsWith('image/png')) {
        return true;
    }

    return url.pathname.toLowerCase().endsWith('.png');
}

function parseFilenameFromContentDisposition(contentDisposition) {
    const header = String(contentDisposition || '').trim();
    if (!header) {
        return null;
    }

    const utf8Match = header.match(/filename\*=UTF-8''([^;]+)/i);
    if (utf8Match && utf8Match[1]) {
        try {
            return decodeURIComponent(utf8Match[1]);
        } catch {
            return utf8Match[1];
        }
    }

    const basicMatch = header.match(/filename="?([^";]+)"?/i);
    if (basicMatch && basicMatch[1]) {
        return basicMatch[1];
    }

    return null;
}

function sanitizeFileName(fileName) {
    const fallback = 'shared-character.png';
    const raw = String(fileName || '').trim();
    const sanitized = raw
        .replace(/[\/\\:*?"<>|\u0000-\u001f]/g, '_')
        .replace(/[. ]+$/g, '')
        .trim();

    if (!sanitized) {
        return fallback;
    }

    return sanitized.toLowerCase().endsWith('.png') ? sanitized : `${sanitized}.png`;
}

export function registerContentRoutes(router, context, { jsonResponse }) {
    router.post('/api/content/importURL', async ({ body }) => {
        const targetUrl = parseUrl(body?.url);
        if (!targetUrl) {
            return jsonResponse({ error: 'Invalid import URL' }, 400);
        }

        const protocol = targetUrl.protocol.toLowerCase();
        if (protocol !== 'http:' && protocol !== 'https:') {
            return jsonResponse({ error: 'Unsupported URL protocol' }, 400);
        }

        if (!isWhitelistedExternalImportUrl(targetUrl)) {
            return jsonResponse({ error: 'Import URL is not whitelisted' }, 403);
        }

        let downloadResult;
        try {
            downloadResult = await context.safeInvoke('download_external_import_url', {
                url: targetUrl.toString(),
            });
        } catch {
            return jsonResponse({ error: 'Unable to download external content' }, 502);
        }

        const contentType = String(downloadResult?.mimeType || '');
        if (!isPngResponse(targetUrl, contentType)) {
            return jsonResponse({ error: 'Only PNG imports are supported' }, 415);
        }

        const rawFileName = String(downloadResult?.fileName || '').trim();
        const responseFileName = parseFilenameFromContentDisposition(rawFileName)
            || rawFileName
            || decodeURIComponent(targetUrl.pathname.split('/').pop() || '')
            || 'shared-character.png';
        const fileName = sanitizeFileName(responseFileName);
        const bytes = normalizeBinaryPayload(downloadResult?.data);

        return new Response(bytes, {
            status: 200,
            headers: {
                'Content-Type': 'image/png',
                'Content-Disposition': `attachment; filename="${fileName}"`,
                'X-Custom-Content-Type': 'character',
            },
        });
    });

    router.post('/api/content/importUUID', async () => {
        return jsonResponse(
            { error: 'UUID import is not supported in this Tauri build. Use a direct URL.' },
            501,
        );
    });
}
