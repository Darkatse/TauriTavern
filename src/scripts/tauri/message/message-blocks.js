// @ts-check

// Elements of the HTML standard. Showdown puts any other tag inside a paragraph, and the parser closes it with the
// paragraph, so only these can hold content across a blank line.
const HTML_ELEMENTS = new Set(`a abbr acronym address applet area article aside audio b base basefont bdi bdo big blink
    blockquote body br button canvas caption center cite code col colgroup data datalist dd del details dfn dialog dir div
    dl dt em embed fieldset figcaption figure font footer form frame frameset h1 h2 h3 h4 h5 h6 head header hgroup hr html
    i iframe img input ins kbd label legend li link main map mark marquee math menu meta meter nav nobr noembed noframes
    noscript object ol optgroup option output p param picture plaintext pre progress q rb rp rt rtc ruby s samp script
    search section select slot small source span strike strong style sub summary sup svg table tbody td template textarea
    tfoot th thead time title tr track tt u ul var video wbr xmp`.split(/\s+/));
const VOID_ELEMENTS = new Set('area base basefont br col embed frame hr img input link meta param source track wbr'.split(' '));
const RAW_TEXT_ELEMENTS = new Set('iframe noembed noframes noscript plaintext script style textarea title xmp'.split(' '));
/** @type {Map<string, RegExp>} */
const rawTextCloses = new Map();

const LINK_DEFINITION = /^ {0,3}\[[^\]]+\]:/m;
// Showdown's fences: the opening line holds the fence and at most an info word, and a line starting with the same
// fence closes it.
const OPENING_FENCE = / {0,3}(`{3,}|~{3,}) *[^\s`~]*(?=\n|$)/y;
const FENCE_INDENT = / {0,3}/y;
const BLANK_LINE = /[ \t]*(?=\n|$)/y;
// Indented, quoted and list item lines continue the block before them.
const CONTINUES_BLOCK = /[ \t>]|[*+-][ \t]|\d+[.)][ \t]/y;
// The self-closing slash does not close an HTML element.
const TAG = /<!--[\s\S]*?(?:-->|$)|<(\/?)([A-Za-z][\w:-]*)(?:[^>"']|"[^"]*"|'[^']*')*>/g;

/**
 * Splits formatted message text into top-level blocks that format the same on their own as within the text. A block
 * ends at a blank line followed by a line that opens a block, outside code fences, HTML elements, comments, raw text
 * and the given spans. Link reference definitions apply to the whole text, which is then one block. The blocks
 * joined by line breaks are the text.
 *
 * @param {string} text
 * @param {Array<[number, number]>} spans Ranges, end exclusive, that no boundary may cut
 * @returns {string[]}
 */
export function splitMessageBlocks(text, spans) {
    if (LINK_DEFINITION.test(text)) {
        return [text];
    }
    const { starts, fences } = scanLines(text);
    const blocks = [];
    let from = 0;
    for (const start of outsideSpans(outsideHtml(text, starts, fences), spans)) {
        blocks.push(text.slice(from, start - 1));
        from = start;
    }
    blocks.push(text.slice(from));
    return blocks;
}

/**
 * Lines after a blank line that open a block outside fences, and the fenced ranges.
 * @param {string} text
 */
function scanLines(text) {
    /** @type {number[]} */
    const starts = [];
    /** @type {Array<[number, number]>} */
    const fences = [];
    let fence = '';
    let fenceStart = 0;
    let blank = false;
    for (let at = 0; at <= text.length;) {
        const newline = text.indexOf('\n', at);
        const end = newline === -1 ? text.length : newline;
        if (fence) {
            FENCE_INDENT.lastIndex = at;
            FENCE_INDENT.test(text);
            if (text.startsWith(fence, FENCE_INDENT.lastIndex)) {
                fences.push([fenceStart, end]);
                fence = '';
            }
            blank = false;
        } else {
            OPENING_FENCE.lastIndex = at;
            const opening = OPENING_FENCE.exec(text);
            BLANK_LINE.lastIndex = at;
            const lineBlank = !opening && BLANK_LINE.test(text);
            CONTINUES_BLOCK.lastIndex = at;
            if (blank && !lineBlank && (opening || !CONTINUES_BLOCK.test(text))) {
                starts.push(at);
            }
            if (opening) {
                fence = /** @type {string} */ (opening[1]);
                fenceStart = at;
            }
            blank = lineBlank;
        }
        at = end + 1;
    }
    if (fence) {
        fences.push([fenceStart, text.length]);
    }
    return { starts, fences };
}

/**
 * The block starts where no element, comment or raw text is open; tags inside fences are text.
 * @param {string} text
 * @param {number[]} starts
 * @param {Array<[number, number]>} fences
 */
function outsideHtml(text, starts, fences) {
    /** @type {string[]} */
    const open = [];
    /** @param {number} from */
    const nextTag = from => {
        TAG.lastIndex = from;
        for (let tag; (tag = TAG.exec(text));) {
            const at = tag.index;
            const fence = fences.find(([fenceStart, fenceEnd]) => fenceStart <= at && at < fenceEnd);
            if (!fence) return tag;
            TAG.lastIndex = fence[1];
        }
        return null;
    };
    /** @param {RegExpExecArray} tag @returns {number} Where the tag's effect on the text ends */
    const apply = tag => {
        const end = tag.index + tag[0].length;
        const name = tag[2]?.toLowerCase();
        if (!name || !HTML_ELEMENTS.has(name) || VOID_ELEMENTS.has(name)) {
            return end;
        }
        if (tag[1]) {
            const at = open.lastIndexOf(name);
            if (at !== -1) open.length = at;
        } else if (RAW_TEXT_ELEMENTS.has(name)) {
            return rawTextEnd(text, name, end);
        } else {
            open.push(name);
        }
        return end;
    };
    let reach = 0;
    let tag = starts.length ? nextTag(0) : null;
    return starts.filter(start => {
        for (; tag && tag.index < start; tag = nextTag(reach)) {
            reach = apply(tag);
        }
        return reach <= start && open.length === 0;
    });
}

/**
 * @param {string} text
 * @param {string} name
 * @param {number} from
 */
function rawTextEnd(text, name, from) {
    let close = rawTextCloses.get(name);
    if (!close) {
        close = new RegExp(`</${name}[\\s/>]`, 'gi');
        rawTextCloses.set(name, close);
    }
    close.lastIndex = from;
    const match = close.exec(text);
    const end = match ? text.indexOf('>', match.index) : -1;
    return end === -1 ? text.length : end + 1;
}

/**
 * The block starts whose separating line break no span covers.
 * @param {number[]} starts
 * @param {Array<[number, number]>} spans
 */
function outsideSpans(starts, spans) {
    const sorted = spans.slice().sort((a, b) => a[0] - b[0]);
    let next = 0;
    let reach = -1;
    return starts.filter(start => {
        for (let span = sorted[next]; span && span[0] < start; span = sorted[++next]) {
            reach = Math.max(reach, span[1]);
        }
        return reach < start;
    });
}
