function makeRouteKey(method, path) {
    return `${String(method || '*').toUpperCase()} ${path}`;
}

export function createRouteRegistry() {
    const routes = new Map();

    function register(method, path, handler) {
        routes.set(makeRouteKey(method, path), handler);
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

            return null;
        },
    };
}
