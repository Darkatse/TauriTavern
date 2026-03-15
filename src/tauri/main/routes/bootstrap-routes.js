export function registerBootstrapRoutes(router, context, { jsonResponse }) {
    router.post('/api/bootstrap', async () => {
        const snapshot = await context.safeInvoke('get_bootstrap_snapshot');
        const normalizedCharacters = snapshot.characters.map((character) => context.normalizeCharacter(character));

        return jsonResponse({
            settings: snapshot.settings,
            characters: normalizedCharacters,
            groups: snapshot.groups,
            avatars: snapshot.avatars,
            secret_state: snapshot.secret_state,
        });
    });
}
