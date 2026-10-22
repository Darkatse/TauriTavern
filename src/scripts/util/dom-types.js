// Nodes adopted from script iframes keep their original JavaScript prototypes.
// Identify DOM types independently of the window that created them.

/** @param {any} value @returns {value is Element} */
export function isElement(value) {
    return value?.nodeType === 1;
}

/** @param {any} value @returns {value is HTMLElement} */
export function isHTMLElement(value) {
    return isElement(value) && value.namespaceURI === 'http://www.w3.org/1999/xhtml';
}
