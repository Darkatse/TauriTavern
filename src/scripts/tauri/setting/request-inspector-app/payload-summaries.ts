import { type Outline, type OutlineMessage, type OutlineRow, requestMessages } from './outline';

export type PayloadSummaries = {
    /** What a closed node is, by JSON Pointer into the request and the response body. */
    request: Map<string, string>;
    response: Map<string, string>;
    /** The containers that hold the messages, open at first. */
    open: string[];
};

/**
 * The payload tree does not know wire formats. The outline already says what
 * the parts are, so its lines name the tree's closed nodes.
 */
export function payloadSummaries(outline: Outline): PayloadSummaries {
    const describe = (messages: OutlineMessage[], rows: OutlineRow[]): Map<string, string> => {
        const summaries = new Map<string, string>();
        const put = (path: string | null, parts: (string | undefined)[]) => {
            const text = parts.filter(Boolean).join(' · ');
            if (path !== null && text && !summaries.has(path)) {
                summaries.set(path, text);
            }
        };
        for (const message of messages) {
            put(message.path, [message.role, message.rows[0]?.name, message.rows[0]?.lead]);
            for (const row of message.rows) {
                put(row.path, [row.name, row.lead]);
            }
        }
        for (const row of rows) {
            put(row.path, [row.name]);
        }
        return summaries;
    };

    const messages = requestMessages(outline);
    const tools = outline.head.flatMap(row => row.body?.kind === 'rows' ? row.body.rows : []);
    const parents = messages.flatMap(({ path }) => path === null ? [] : [path.slice(0, path.lastIndexOf('/'))]);
    return {
        request: describe(messages, tools),
        response: describe(outline.response?.messages ?? [], []),
        open: [...new Set(parents)].filter(Boolean),
    };
}
