// @ts-check

/**
 * @param {ReturnType<import('../backend.js').createBackend>} backend
 * @param {typeof fetch} fetchResource
 * @param {() => Promise<any>} loadDefaults
 * @returns {import('../install.js').CommandHandlers}
 */
export function createExtensionCommands({ json, send }, fetchResource, loadDefaults) {
    /** @param {string} path */
    async function readTemplate(path) {
        const response = await fetchResource(path);
        if (!response.ok) throw new Error(`Cannot read frontend template ${path}: ${response.status}`);
        return response.text();
    }
    return {
        get_extensions: () => json('/api/extensions/discover', undefined, 'GET').map(/** @param {{ name: string, type: string }} extension */ extension => ({
            name: extension.name, extension_type: extension.type.charAt(0).toUpperCase() + extension.type.slice(1),
        })),
        read_frontend_template: ({ name }) => readTemplate(`/scripts/templates/${name}`),
        read_frontend_extension_template: ({ extension, name }) => readTemplate(`/scripts/extensions/${extension}/${name}.html`),
        open_extension_store_entry: () => null,
        list_mcp_servers: () => ({ servers: [], storageIssues: [] }),
        list_agent_profiles: async () => structuredClone((await loadDefaults()).agentProfiles),
        list_skills: () => [],
        // Accept page-owned tool declarations; Agent execution remains unsupported.
        register_agent_extension_tool: () => undefined,
        list_sprites: ({ dto }) => json(`/api/sprites/get?name=${encodeURIComponent(dto.name)}`, undefined, 'GET'),
        // Like native, failed statuses are returned for the SD extension to handle.
        sd_handle: ({ path, body }) => {
            const response = send(`/api/sd/${path}`, JSON.stringify(body));
            const text = /** @type {string | null} */ (response.body);
            if (!text) return { status: response.status, kind: 'empty', body: null };
            return response.headers['content-type']?.includes('json')
                ? { status: response.status, kind: 'json', body: JSON.parse(text) }
                : { status: response.status, kind: 'text', body: text };
        },
    };
}
