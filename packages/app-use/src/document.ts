/** A page of refs belongs to one document, reached through these outer-to-inner frames. */
export type DocumentScope = { document: Document; frames: HTMLIFrameElement[] };

export function mainDocumentScope(): DocumentScope {
    return { document, frames: [] };
}

export function isElement(node: Node): node is Element {
    return node.nodeType === Node.ELEMENT_NODE;
}

export function isText(node: Node): node is Text {
    return node.nodeType === Node.TEXT_NODE;
}

// Nodes adopted from another window keep their original prototypes. Do not use instanceof here.
export function isHTMLElement(element: Element): element is HTMLElement {
    return element.namespaceURI === 'http://www.w3.org/1999/xhtml';
}

export function isHtmlTag<K extends keyof HTMLElementTagNameMap>(element: Element, tag: K): element is HTMLElementTagNameMap[K] {
    return isHTMLElement(element) && element.localName === tag;
}

export function documentWindow(doc: Document) {
    const view = doc.defaultView;
    if (!view) throw new Error('This page is no longer active. Call app.snapshot with {} to observe the main page again.');
    return view;
}

export function isScopeCurrent(scope: DocumentScope): boolean {
    let current: Document | null = document;
    for (const frame of scope.frames) {
        if (!frame.isConnected || frame.ownerDocument !== current) return false;
        current = frame.contentDocument;
        if (!current) return false;
    }
    return current === scope.document;
}

export function isInScope(element: Element, scope: DocumentScope): boolean {
    // Old nodes can remain connected to an old Document after iframe navigation.
    return element.isConnected && element.ownerDocument === scope.document && isScopeCurrent(scope);
}

export function requireCurrentScope(scope: DocumentScope) {
    if (!isScopeCurrent(scope)) {
        throw new Error('The embedded page was reloaded, removed, or is no longer accessible. Call app.snapshot with {} and enter it again using its new ref.');
    }
}
