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

    function register(method, path, handler) {
        const wildcardPrefix = normalizeWildcardPrefix(path);
        if (wildcardPrefix !== null) {
            wildcardRoutes.push({
                method: String(method || '*').toUpperCase(),
                prefix: wildcardPrefix,
                handler,
            });
            return;
        }

        routes.set(makeRouteKey(method, path), handler);
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

    return {
        get(path, handler) {
            register('GET', path, handler);
        },
        post(path, handler) {
            register('POST', path, handler);
        },
        all(path, handler) {
            register('*', path, handler);
        },
        async handle(request) {
            const specific = routes.get(makeRouteKey(request.method, request.path));
            if (specific) {
                return specific(request);
            }

            const wildcard = routes.get(makeRouteKey('*', request.path));
            if (wildcard) {
                return wildcard(request);
            }

            const wildcardRoute = findWildcardHandler(request.method, request.path);
            if (wildcardRoute) {
                const wildcard = request.path.slice(wildcardRoute.prefix.length);
                return wildcardRoute.handler({ ...request, wildcard });
            }

            return null;
        },
    };
}
