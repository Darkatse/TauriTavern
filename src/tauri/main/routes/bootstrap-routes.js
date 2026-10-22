export function registerBootstrapRoutes(router, context, { jsonResponse }) {
    router.post('/api/bootstrap', async () => {
        const snapshot = await context.safeInvoke('get_bootstrap_metadata');
        return jsonResponse({
            ...snapshot,
            characters: snapshot.characters.map(character => context.normalizeCharacter(character)),
        });
    });
}
