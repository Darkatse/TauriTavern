// @ts-check

import { requirementsOf } from './pattern-requirements.js';

/** Never matches, adds no capture group, keeps alternative order. */
const DEAD_ALTERNATIVE = '(?!)';
const CACHE_LIMIT = 1000;

/**
 * Bounded memo; entries are dropped wholesale when full since recomputing is cheap.
 * @template V
 * @returns {(key: string, compute: () => V) => V}
 */
function memo() {
    /** @type {Map<string, V>} */
    const entries = new Map();
    return (key, compute) => {
        if (!entries.has(key)) {
            if (entries.size >= CACHE_LIMIT) {
                entries.clear();
            }
            entries.set(key, compute());
        }
        return /** @type {V} */ (entries.get(key));
    };
}

/** @type {(key: string, compute: () => ReturnType<typeof requirementsOf>) => ReturnType<typeof requirementsOf>} */
const analyses = memo();
/** @type {(key: string, compute: () => RegExp) => RegExp} */
const compiled = memo();

/**
 * @typedef {(...args: any[]) => string} Replacer Called like the function form of `String.prototype.replace`
 */

/**
 * Plans `text.replace(regex, replacer)` with the work required literals rule out skipped.
 *
 * An alternative whose required literals are absent cannot match, so it is
 * neutralised with `(?!)`; when none can match there is nothing to run and the
 * plan is null. In a global replace no match can start beyond the last
 * occurrence of an alternative's literals, so the search stops there instead
 * of proving the remaining text empty one position at a time.
 *
 * @param {string} text
 * @param {RegExp} regex
 * @returns {((replacer: Replacer) => string) | null}
 */
export function planReplace(text, regex) {
    const alternatives = analyses(regex.source, () => requirementsOf(regex.source));
    if (alternatives === null) {
        return replacer => text.replace(regex, replacer);
    }

    const lastIndexOf = lastIndexFinder(text, regex);
    const canMatchFrom = (/** @type {{ requirements: string[][] }} */ { requirements }, /** @type {number} */ position) =>
        requirements.every(options => options.some(literal => lastIndexOf(literal) >= position));

    const live = alternatives.filter(alternative => canMatchFrom(alternative, 0));
    if (live.length === 0) {
        return null;
    }

    const effective = live.length === alternatives.length ? regex : specialise(regex, alternatives, live);
    if (!regex.global) {
        return replacer => text.replace(effective, replacer);
    }

    return (replacer) => {
        effective.lastIndex = 0;
        let output = '';
        let cursor = 0;
        while (live.some(alternative => canMatchFrom(alternative, effective.lastIndex))) {
            const match = effective.exec(text);
            if (match === null) {
                break;
            }
            output += text.slice(cursor, match.index);
            output += replacer(...match, match.index, text, ...(match.groups ? [match.groups] : []));
            cursor = match.index + match[0].length;
            if (match[0] === '') {
                effective.lastIndex = advanceStringIndex(text, effective.lastIndex, regex.unicode);
            }
        }
        return output + text.slice(cursor);
    };
}

/**
 * `text.replace(regex, replacer)` with the work required literals rule out skipped.
 * @param {string} text
 * @param {RegExp} regex
 * @param {Replacer} replacer
 * @returns {string}
 */
export function gatedReplace(text, regex, replacer) {
    return planReplace(text, regex)?.(replacer) ?? text;
}

/**
 * Neutralises the alternatives that cannot match; capture numbering and order are untouched.
 * @param {RegExp} regex
 * @param {NonNullable<ReturnType<typeof requirementsOf>>} alternatives
 * @param {NonNullable<ReturnType<typeof requirementsOf>>} live
 */
function specialise(regex, alternatives, live) {
    let source = '';
    let cursor = 0;
    for (const alternative of alternatives) {
        source += regex.source.slice(cursor, alternative.start)
            + (live.includes(alternative) ? '' : DEAD_ALTERNATIVE)
            + regex.source.slice(alternative.start, alternative.end);
        cursor = alternative.end;
    }
    source += regex.source.slice(cursor);
    return compiled(`${regex.flags}/${source}`, () => new RegExp(source, regex.flags));
}

/**
 * Last start offset of each literal under the regex's own case semantics, memoised per call.
 * @param {string} text
 * @param {RegExp} regex
 * @returns {(literal: string) => number}
 */
function lastIndexFinder(text, regex) {
    /** @type {Map<string, number>} */
    const found = new Map();
    return (literal) => {
        if (!found.has(literal)) {
            found.set(literal, regex.ignoreCase ? lastCaseInsensitiveIndex(text, literal, regex.unicode) : text.lastIndexOf(literal));
        }
        return /** @type {number} */ (found.get(literal));
    };
}

/**
 * Case folding follows the engine by probing with a literal regex under the same flags.
 * Occurrences may overlap, so the probe steps one code unit at a time.
 * @param {string} text
 * @param {string} literal
 * @param {boolean} unicode
 */
function lastCaseInsensitiveIndex(text, literal, unicode) {
    const flags = unicode ? 'giu' : 'gi';
    const probe = compiled(`${flags}/${literal}`, () => new RegExp(literal.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), flags));
    probe.lastIndex = 0;
    let index = -1;
    for (let match = probe.exec(text); match !== null; match = probe.exec(text)) {
        index = match.index;
        probe.lastIndex = match.index + 1;
    }
    return index;
}

/**
 * `AdvanceStringIndex` from the spec: past an empty match, `u` mode steps over a whole code point.
 * @param {string} text
 * @param {number} index
 * @param {boolean} unicode
 */
function advanceStringIndex(text, index, unicode) {
    return index + (unicode && (text.codePointAt(index) ?? 0) > 0xFFFF ? 2 : 1);
}
