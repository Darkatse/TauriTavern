// @ts-check

import { morphdom } from '../../../lib.js';
import { segmentExistingTextInElement } from '../../util/stream-fadein.js';

// Ids repeat across messages, so they do not identify nodes.
const MORPH_OPTIONS = { getNodeKey: () => undefined };
const WRITES = { childList: true, subtree: true, characterData: true, attributes: true };

/**
 * Writes successive versions of an element's content, given as blocks of HTML joined by line breaks. A block equal
 * to the one in the same place of the previous write keeps its nodes; the others are morphed in place, which keeps
 * their unchanged nodes too. After anything else changes the element, the next write morphs every block.
 */
export function createBlockContent() {
    /** @type {HTMLElement | null} */
    let element = null;
    /** @type {string[]} */
    let blocks = [];
    /** @type {ChildNode[][]} */
    let nodes = [];
    let foreign = true;
    // Watches the element from one write to the next; the first change by anyone else ends it.
    const observer = new MutationObserver(() => {
        foreign = true;
        observer.disconnect();
    });

    /**
     * @param {HTMLElement} target
     * @returns {boolean} Whether the element holds what the last write left, untouched since
     */
    function owns(target) {
        if (observer.takeRecords().length) {
            foreign = true;
            observer.disconnect();
        }
        return target === element && !foreign;
    }

    /**
     * @param {HTMLElement} target
     * @param {string[]} next
     * @param {{ fadeIn?: boolean }} [options]
     * @returns {{ content: HTMLElement; commit: () => HTMLElement } | null} Null when the element already holds them
     */
    function prepare(target, next, { fadeIn = false } = {}) {
        const owned = owns(target);
        const kept = owned ? blocks : [];
        const same = (/** @type {number} */ i) => i < kept.length && i < next.length && next[i] === kept[i];
        if (owned && kept.length === next.length && next.every((_, i) => same(i))) {
            return null;
        }
        // The nodes of the blocks this write changes.
        const content = /** @type {HTMLElement} */ (target.cloneNode(false));
        /** @param {number} i */
        const parse = i => {
            const holder = /** @type {HTMLElement} */ (target.cloneNode(false));
            holder.innerHTML = (i ? '\n' : '') + next[i];
            if (fadeIn) segmentExistingTextInElement(holder);
            const parsed = [...holder.childNodes];
            content.append(...parsed);
            return parsed;
        };
        /** @type {Array<ChildNode[] | null>} */
        const fresh = next.map((_, i) => same(i) ? null : parse(i));

        function commit() {
            /** @type {ChildNode[][]} */
            const result = new Array(next.length);
            /** @param {ChildNode[]} from @param {number} start @param {number} stop @param {ChildNode | null} before */
            const place = (from, start, stop, before) => {
                const groups = /** @type {ChildNode[][]} */ (fresh.slice(start, stop));
                const placed = morphRun(target, from, groups.flat(), before);
                let at = 0;
                groups.forEach((group, k) => {
                    result[start + k] = placed.slice(at, at + group.length);
                    at += group.length;
                });
            };
            if (owned && owns(target)) {
                const end = Math.max(kept.length, next.length);
                for (let i = 0; i < end;) {
                    if (same(i)) {
                        result[i] = /** @type {ChildNode[]} */ (nodes[i]);
                        i++;
                        continue;
                    }
                    let j = i + 1;
                    while (j < end && !same(j)) j++;
                    place(nodes.slice(i, Math.min(j, kept.length)).flat(), i, Math.min(j, next.length), nodes[j]?.[0] ?? null);
                    i = j;
                }
            } else {
                next.forEach((_, i) => { fresh[i] ??= parse(i); });
                place([...target.childNodes], 0, next.length, null);
            }
            element = target;
            blocks = next;
            nodes = result;
            foreign = false;
            observer.takeRecords();
            observer.observe(target, WRITES);
            return target;
        }

        return { content, commit };
    }

    return { owns, prepare };
}

/**
 * Puts `to` in place of the consecutive children `from`, which end before `before`. A node of the same name as the
 * one in its place is morphed into it, and that node stays.
 * @param {HTMLElement} parent
 * @param {ChildNode[]} from
 * @param {ChildNode[]} to
 * @param {ChildNode | null} before
 * @returns {ChildNode[]} The children now in place of `to`
 */
function morphRun(parent, from, to, before) {
    const placed = [];
    for (let k = 0; k < Math.max(from.length, to.length); k++) {
        const old = from[k];
        const node = to[k];
        if (old && node && old.nodeType === node.nodeType && old.nodeName === node.nodeName) {
            if (old.nodeType === Node.ELEMENT_NODE) {
                morphdom(old, node, MORPH_OPTIONS);
            } else if (old.nodeValue !== node.nodeValue) {
                old.nodeValue = node.nodeValue;
            }
            placed.push(old);
            continue;
        }
        if (node) {
            parent.insertBefore(node, old ?? before);
            placed.push(node);
        }
        old?.remove();
    }
    return placed;
}
