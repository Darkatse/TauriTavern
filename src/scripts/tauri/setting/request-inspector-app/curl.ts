import type { PartView, RequestDescription, RequestView } from './RequestInspectorContract';

/** What the adapter writes in place of a credential. */
const REDACTED = '[redacted]';

/** The stored body holds a placeholder where inline media was, so a replay sends the placeholder. */
export function hasInlineMedia(request: RequestView): boolean {
    const inline = (parts: PartView[]): boolean => parts.some(part => part.type === 'toolResult'
        ? inline(part.parts)
        : part.type === 'media' && part.encodedLength !== undefined);
    return inline(request.system) || request.messages.some(message => inline(message.parts));
}

/**
 * A shell command that replays the described request. Credentials are not in
 * the description; each becomes a prefixed shell variable for the reader to
 * set. A WebSocket request has no cURL form.
 */
export function buildCurl(http: RequestDescription, body: string): string | null {
    if (http.transport !== 'http') {
        return null;
    }
    const end = delimiter(body);
    const lines = [
        `curl -X ${http.method} ${quoteUrl(http.url)}`,
        ...http.headers.map(({ name, value }) => `-H ${value === REDACTED
            ? `"${escapeDouble(`${name}: `)}$${variable('HEADER', name)}"`
            : quoteSingle(`${name}: ${value}`)}`),
        `--data-binary @- <<${quoteSingle(end)}`,
    ];
    return `${lines.join(' \\\n  ')}\n${body}\n${end}\n`;
}

function quoteUrl(url: string): string {
    const query = url.indexOf('?');
    if (query < 0 || !url.includes(`=${REDACTED}`)) {
        return quoteSingle(url);
    }
    const parameters = url.slice(query + 1).split('&').map(parameter => {
        const name = parameter.slice(0, parameter.indexOf('='));
        return parameter === `${name}=${REDACTED}`
            ? `${escapeDouble(name)}=$${variable('QUERY', name)}`
            : escapeDouble(parameter);
    });
    return `"${escapeDouble(url.slice(0, query + 1))}${parameters.join('&')}"`;
}

function variable(prefix: string, name: string): string {
    return `${prefix}_${name.toUpperCase().replace(/[^A-Z0-9]+/gu, '_')}`;
}

function quoteSingle(value: string): string {
    return `'${value.replaceAll('\'', '\'\\\'\'')}'`;
}

function escapeDouble(value: string): string {
    return value.replace(/[\\"$`]/gu, '\\$&');
}

/** A heredoc ends at the first line equal to its delimiter. */
function delimiter(body: string): string {
    const lines = new Set(body.split('\n'));
    let name = 'TT_REQUEST_BODY';
    for (let attempt = 1; lines.has(name); attempt++) {
        name = `TT_REQUEST_BODY_${attempt}`;
    }
    return name;
}
