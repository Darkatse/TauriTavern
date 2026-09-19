// @ts-check

/**
 * Required-literal analysis for ECMAScript regex sources.
 *
 * For each top-level alternative, derives literals that every match of that
 * alternative must contain. Requirements are a conjunction of choices:
 * `[['<think>'], ['</think>', '</thinking>']]` reads "contains `<think>`, and
 * contains `</think>` or `</thinking>`".
 *
 * Two rules keep the analysis sound without ever looking at flags:
 * - anything not understood contributes nothing (over-approximating "this
 *   alternative might match" is always safe);
 * - only literals whose meaning is identical with and without the `u` flag are
 *   extracted, so `\u{…}`, `\p{…}` and surrogate halves are opaque.
 *
 * @typedef {{ start: number, end: number, requirements: string[][] }} Alternative
 *   `start`/`end` delimit the alternative's text inside the source.
 *
 * @typedef {{ type: 'char', value: string } | { type: 'assertion' } | { type: 'opaque' } | Group} Atom
 * @typedef {{ type: 'group', alternatives: ParsedAlternative[] }} Group
 * @typedef {Atom | { type: 'repeat', min: number, atom: Atom }} Term
 * @typedef {{ start: number, end: number, terms: Term[] }} ParsedAlternative
 */

const UNANALYSABLE = Symbol('unanalysable');
/** @type {Atom} */
const ASSERTION = { type: 'assertion' };
/** @type {Atom} */
const OPAQUE = { type: 'opaque' };
const BRACED_QUANTIFIER = /\{(\d+)(?:,\d*)?\}/y;

/**
 * @param {string} source `RegExp#source`
 * @returns {Alternative[] | null} Top-level alternatives, or null when the source is not understood.
 */
export function requirementsOf(source) {
    let alternatives;
    try {
        alternatives = new Parser(source).parse();
    } catch (error) {
        if (error !== UNANALYSABLE) {
            throw error;
        }
        return null;
    }

    return unwrapSoleGroup(alternatives).map(({ start, end, terms }) => ({ start, end, requirements: alternativeRequirements(terms) }));
}

/**
 * A pattern that is a single group, e.g. `(?:a|b)`, alternates one level down.
 * @param {ParsedAlternative[]} alternatives
 * @returns {ParsedAlternative[]}
 */
function unwrapSoleGroup(alternatives) {
    const [first, ...rest] = alternatives;
    const [term, ...more] = first?.terms ?? [];
    if (rest.length > 0 || more.length > 0 || term?.type !== 'group') {
        return alternatives;
    }
    return unwrapSoleGroup(term.alternatives);
}

/**
 * @param {Term[]} terms
 * @returns {string[][]}
 */
function alternativeRequirements(terms) {
    /** @type {string[][]} */
    const requirements = [];
    let run = '';
    const endRun = () => {
        if (run) {
            requirements.push([run]);
        }
        run = '';
    };

    for (const term of terms) {
        switch (term.type) {
            case 'char':
                run += term.value;
                break;
            case 'assertion':
                // Zero-width: the surrounding literal stays contiguous.
                break;
            case 'group':
                endRun();
                requirements.push(...groupRequirements(term));
                break;
            case 'repeat':
                if (term.min === 0) {
                    endRun();
                } else if (term.atom.type === 'char') {
                    // The first repetition touches the run before it, the last one the run after.
                    run += term.atom.value;
                    endRun();
                    run = term.atom.value;
                } else {
                    endRun();
                    if (term.atom.type === 'group') {
                        requirements.push(...groupRequirements(term.atom));
                    }
                }
                break;
            default:
                endRun();
        }
    }
    endRun();
    return requirements;
}

/**
 * A match takes exactly one alternative of the group, so one requirement per
 * alternative, merged into a single choice, holds for every match.
 * @param {Group} group
 * @returns {string[][]}
 */
function groupRequirements(group) {
    const perAlternative = group.alternatives.map(alternative => alternativeRequirements(alternative.terms));
    if (perAlternative.length > 1) {
        return perAlternative.every(requirements => requirements.length > 0)
            ? [perAlternative.flatMap(mostSelective)]
            : [];
    }
    return perAlternative.flat();
}

/**
 * The requirement whose weakest option is longest.
 * @param {string[][]} requirements
 */
function mostSelective(requirements) {
    const shortest = (/** @type {string[]} */ options) => Math.min(...options.map(literal => literal.length));
    return requirements.reduce((best, candidate) => (shortest(candidate) > shortest(best) ? candidate : best));
}

/**
 * @param {string} value
 * @returns {Atom}
 */
function char(value) {
    // Quantifiers repeat a surrogate half without `u` but a whole code point with it.
    return /[\uD800-\uDFFF]/.test(value) ? OPAQUE : { type: 'char', value };
}

class Parser {
    /** @param {string} source */
    constructor(source) {
        this.source = source;
        this.pos = 0;
    }

    parse() {
        const alternatives = this.disjunction();
        if (this.pos !== this.source.length) {
            throw UNANALYSABLE;
        }
        return alternatives;
    }

    /** The code unit at the cursor, or '' at the end. */
    peek(offset = 0) {
        return this.source.charAt(this.pos + offset);
    }

    /** @param {string} ch */
    expect(ch) {
        if (this.peek() !== ch) {
            throw UNANALYSABLE;
        }
        this.pos += 1;
    }

    /** Advances past the next `terminator`. @param {string} terminator */
    skipPast(terminator) {
        const end = this.source.indexOf(terminator, this.pos);
        if (end === -1) {
            throw UNANALYSABLE;
        }
        this.pos = end + 1;
    }

    /** @returns {ParsedAlternative[]} */
    disjunction() {
        const alternatives = [];
        for (;;) {
            const start = this.pos;
            const terms = this.alternative();
            alternatives.push({ start, end: this.pos, terms });
            if (this.peek() !== '|') {
                return alternatives;
            }
            this.pos += 1;
        }
    }

    /** @returns {Term[]} */
    alternative() {
        /** @type {Term[]} */
        const terms = [];
        while (this.pos < this.source.length && this.peek() !== '|' && this.peek() !== ')') {
            const atom = this.atom();
            const min = this.quantifier();
            terms.push(min === null ? atom : { type: 'repeat', min, atom });
        }
        return terms;
    }

    /** @returns {Atom} */
    atom() {
        const ch = this.peek();
        switch (ch) {
            case '^':
            case '$':
                this.pos += 1;
                return ASSERTION;
            case '.':
                this.pos += 1;
                return OPAQUE;
            case '[':
                return this.characterClass();
            case '(':
                return this.group();
            case '\\':
                return this.escape();
            case '*':
            case '+':
            case '?':
                throw UNANALYSABLE;
            default:
                this.pos += 1;
                return char(ch);
        }
    }

    /** @returns {number | null} Minimum repeat count, or null when no quantifier follows. */
    quantifier() {
        let min;
        switch (this.peek()) {
            case '*':
            case '?':
                min = 0;
                this.pos += 1;
                break;
            case '+':
                min = 1;
                this.pos += 1;
                break;
            case '{': {
                BRACED_QUANTIFIER.lastIndex = this.pos;
                const braced = BRACED_QUANTIFIER.exec(this.source);
                if (!braced) {
                    return null;
                }
                min = Number(braced[1]);
                this.pos += braced[0].length;
                break;
            }
            default:
                return null;
        }
        if (this.peek() === '?') {
            this.pos += 1;
        }
        return min;
    }

    /** @returns {Atom} */
    characterClass() {
        this.expect('[');
        while (this.peek() !== ']') {
            if (this.peek() === '') {
                throw UNANALYSABLE;
            }
            this.pos += this.peek() === '\\' ? 2 : 1;
        }
        this.pos += 1;
        return OPAQUE;
    }

    /** @returns {Atom} */
    group() {
        this.expect('(');
        let contributes = true;
        if (this.peek() === '?') {
            const marker = this.peek(1);
            this.pos += 2;
            if (marker === '<' && (this.peek() === '=' || this.peek() === '!')) {
                // Lookbehind text may sit before the match start.
                this.pos += 1;
                contributes = false;
            } else if (marker === '<') {
                this.skipPast('>');
            } else if (marker === '!') {
                contributes = false;
            } else if (marker !== ':' && marker !== '=') {
                throw UNANALYSABLE;
            }
        }
        const alternatives = this.disjunction();
        this.expect(')');
        return contributes ? { type: 'group', alternatives } : OPAQUE;
    }

    /** @returns {Atom} */
    escape() {
        this.expect('\\');
        const ch = this.peek();
        this.pos += 1;
        switch (ch) {
            case 'n': return char('\n');
            case 'r': return char('\r');
            case 't': return char('\t');
            case 'f': return char('\f');
            case 'v': return char('\v');
            case 'x': return this.hexChar(2);
            case 'u':
                if (this.peek() !== '{') {
                    return this.hexChar(4);
                }
                // Falls through: a code point with `u`, literal text without.
            case 'p':
            case 'P':
            case 'k':
                if (this.peek() === '{' || this.peek() === '<') {
                    this.skipPast(this.peek() === '{' ? '}' : '>');
                }
                return OPAQUE;
            case 'd': case 'D': case 'w': case 'W': case 's': case 'S':
                return OPAQUE;
            case 'b':
            case 'B':
                return ASSERTION;
            case 'c':
            case '':
                throw UNANALYSABLE;
            default:
                if (/\d/.test(ch)) {
                    // Backreference, or a legacy octal escape.
                    while (/\d/.test(this.peek())) {
                        this.pos += 1;
                    }
                    return OPAQUE;
                }
                return char(ch);
        }
    }

    /** @param {number} digits */
    hexChar(digits) {
        const hex = this.source.slice(this.pos, this.pos + digits);
        if (hex.length !== digits || !/^[0-9a-fA-F]+$/.test(hex)) {
            throw UNANALYSABLE;
        }
        this.pos += digits;
        return char(String.fromCharCode(parseInt(hex, 16)));
    }
}
