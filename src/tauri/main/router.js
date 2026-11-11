// Legacy SillyTavern chat endpoints used by third-party extensions.
const LEGACY_API_PATHS = new Map([
    ['/getallchatsofcharacter', '/api/characters/chats'],
    ['/getchat', '/api/chats/get'],
    ['/getgroupchat', '/api/chats/group/get'],
]);

function makeRouteKey(method, path) {
    return `${String(method || '*').toUpperCase()} ${path}`;
}

function normalizeWildcardPrefix(path) {
    const normalized = String(path || '');
    return normalized.endsWith('*') ? normalized.slice(0, -1) : null;
}

export function createRouteRegistry() {
    const routes = new Map();
    const wildcardRoutes = [];

    function register(method, path, handler, { body = 'json' } = {}) {
        const wildcardPrefix = normalizeWildcardPrefix(path);
        if (wildcardPrefix !== null) {
            wildcardRoutes.push({
                method: String(method || '*').toUpperCase(),
                prefix: wildcardPrefix,
                handler,
                body,
            });
            return;
        }

        routes.set(makeRouteKey(method, path), { handler, body });
    }

    function findWildcardHandler(method, path) {
        const normalizedMethod = String(method || 'GET').toUpperCase();
        const normalizedPath = String(path || '');

        let matchedSpecific = null;
        let matchedWildcard = null;

        for (const route of wildcardRoutes) {
            if (!normalizedPath.startsWith(route.prefix)) {
                continue;
            }

            if (route.method === normalizedMethod) {
                if (!matchedSpecific || route.prefix.length > matchedSpecific.prefix.length) {
                    matchedSpecific = route;
                }
                continue;
            }

            if (route.method === '*') {
                if (!matchedWildcard || route.prefix.length > matchedWildcard.prefix.length) {
                    matchedWildcard = route;
                }
            }
        }

        return matchedSpecific || matchedWildcard;
    }

    function resolve(method, path) {
        path = LEGACY_API_PATHS.get(path) ?? path;
        const specific = routes.get(makeRouteKey(method, path));
        if (specific) {
            return { ...specific, wildcard: '' };
        }

        const wildcard = routes.get(makeRouteKey('*', path));
        if (wildcard) {
            return { ...wildcard, wildcard: '' };
        }

        const wildcardRoute = findWildcardHandler(method, path);
        if (!wildcardRoute) {
            return null;
        }

        return {
            handler: wildcardRoute.handler,
            body: wildcardRoute.body,
            wildcard: path.slice(wildcardRoute.prefix.length),
        };
    }

    return {
        get(path, handler, options) {
            register('GET', path, handler, options);
        },
        post(path, handler, options) {
            register('POST', path, handler, options);
        },
        all(path, handler, options) {
            register('*', path, handler, options);
        },
        bodyMode(method, path) {
            return resolve(method, path)?.body ?? 'json';
        },
        canHandle(method, path) {
            const normalizedMethod = String(method || 'GET').toUpperCase();
            const normalizedPath = String(path || '');
            return resolve(normalizedMethod, normalizedPath) !== null;
        },
        async handle(request) {
            const normalizedMethod = String(request.method || 'GET').toUpperCase();
            const normalizedPath = String(request.path || '');
            const resolved = resolve(normalizedMethod, normalizedPath);
            if (resolved) {
                return resolved.handler({ ...request, wildcard: resolved.wildcard });
            }

            return null;
        },
    };
}
