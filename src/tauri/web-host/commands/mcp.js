// @ts-check

/**
 * MCP registrations, kept in the store document. A new server starts paused, as on native; using one is not supported.
 * @param {import('../install.js').HostStatePart} part
 * @returns {import('../install.js').CommandHandlers}
 */
export function createMcpCommands(part) {
    /** @returns {any[]} */
    const servers = () => part.load() ?? [];
    return {
        list_mcp_servers: () => ({ servers: structuredClone(servers()), storageIssues: [] }),
        create_mcp_server: ({ dto }) => {
            const server = {
                id: crypto.randomUUID(), displayName: dto.displayName, endpoint: dto.endpoint, headers: dto.headers ?? {},
                protocolVersion: dto.protocolVersion ?? 'auto', state: 'paused', toolPermissions: {}, toolDescriptionOverrides: {},
            };
            part.save([...servers(), server]);
            return structuredClone(server);
        },
    };
}
