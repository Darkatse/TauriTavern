export const appToolIds = {
    snapshot: 'app.snapshot',
    interact: 'app.interact',
    evaluate: 'app.evaluate',
    readLogs: 'app.read_logs',
} as const;

// TT and Studio both alias a tool ID by replacing its dots with underscores.
function modelAlias(id: string): string {
    return id.replaceAll('.', '_');
}

export const appToolAliases = {
    snapshot: modelAlias(appToolIds.snapshot),
    interact: modelAlias(appToolIds.interact),
    evaluate: modelAlias(appToolIds.evaluate),
    readLogs: modelAlias(appToolIds.readLogs),
} as const;
