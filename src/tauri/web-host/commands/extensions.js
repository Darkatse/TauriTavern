// @ts-check

/**
 * @param {ReturnType<import('../backend.js').createBackend>} backend
 * @param {typeof fetch} fetchResource
 * @param {() => Promise<any>} loadDefaults
 * @returns {import('../install.js').CommandHandlers}
 */
export function createExtensionCommands({ json }, fetchResource, loadDefaults) {
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
    };
}
