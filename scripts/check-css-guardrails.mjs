import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import postcss from 'postcss';
import selectorParser from 'postcss-selector-parser';

const TAG = '[css-guardrails]';
const repoRoot = fileURLToPath(new URL('..', import.meta.url));

const isVendored = file => file.startsWith('src/lib/') || file.endsWith('.min.css');

const RULES = {
    unnarrowedSibling: {
        title: 'Nothing after + or ~ that narrows the siblings',
        why: `Chromium chooses which siblings to restyle from the part after + or ~ alone and ignores the
ancestors. With a bare tag, * or only :not(...) there, a state change or node insertion anywhere
on the page restyles the following siblings, even when the rule matches nothing.`,
        fix: [
            '- Give the element a class and use it after the combinator: input:checked + .switch-track',
            '- Or select the element by its own class or id, without the sibling.',
            '- For spacing between siblings, use gap on the parent or > :not(:first-child).',
            '- If the rule matches nothing, delete it.',
        ],
    },
    stateHasBeforeSibling: {
        title: ':has() that checks an element state, followed by + or ~',
        why: 'In WebKit, any change of that state anywhere on the page rechecks the whole page.',
        fix: [
            '- Anchor :has() on the common parent and reach the target as a descendant:',
            '    #parent:has(#b:checked) #c   instead of   #a:has(#b:checked) ~ #c',
        ],
    },
};

// Chromium narrows the siblings it restyles by a class, id or attribute in the compound after
// `+` or `~`, or by `:is()`/`:where()` when every argument ends in one. A tag, `*`, `&` or any
// other pseudo-class there, `:not(...)` included, narrows nothing.
const NARROWING = new Set(['class', 'id', 'attribute']);
const FORWARDING = new Set([':is', ':where']);

// Pseudo-classes for interaction, focus, form and open state.
const STATES = new Set([
    ':active', ':autofill', ':checked', ':default', ':disabled', ':enabled', ':focus', ':focus-visible',
    ':focus-within', ':hover', ':in-range', ':indeterminate', ':invalid', ':modal', ':open',
    ':out-of-range', ':placeholder-shown', ':popover-open', ':read-only', ':read-write', ':target',
    ':user-invalid', ':user-valid', ':valid', ':-webkit-autofill',
]);

const rightmostCompound = complex =>
    complex.nodes.slice(complex.nodes.findLastIndex(node => node.type === 'combinator') + 1);

const narrows = compound => compound.some(node => NARROWING.has(node.type)
    || (node.type === 'pseudo' && FORWARDING.has(node.value.toLowerCase()) && node.nodes.length > 0
        && node.nodes.every(argument => narrows(rightmostCompound(argument)))));

function firstState(has) {
    let state = null;
    has.walkPseudos(pseudo => {
        if (STATES.has(pseudo.value.toLowerCase())) {
            state = pseudo.value;
            return false;
        }
    });
    return state;
}

// Blanking comments keeps every column in place and leaves no comment nodes between compounds.
const blankComments = text => text.replace(/\/\*[\s\S]*?\*\//gu, comment => comment.replace(/[^\n]/gu, ' '));

const isSiblingCombinator = node => node.type === 'combinator' && (node.value === '+' || node.value === '~');

/** Every rule break in one selector, as `{ rule, node, length, note }`. */
function findBreaks(selector) {
    const breaks = [];
    for (const { nodes } of selector.nodes) {
        let compoundStart = 0;
        nodes.forEach((node, index) => {
            if (node.type !== 'combinator') return;
            if (isSiblingCombinator(node)) {
                const hasPseudos = nodes.slice(compoundStart, index)
                    .filter(left => left.type === 'pseudo' && left.value.toLowerCase() === ':has');
                for (const has of hasPseudos) {
                    const state = firstState(has);
                    if (state) {
                        breaks.push({
                            rule: 'stateHasBeforeSibling', node: has, length: String(has).trim().length,
                            note: `:has() checks ${state} before "${node.value}"`,
                        });
                    }
                }
                const end = nodes.findIndex((next, at) => at > index && next.type === 'combinator');
                const subject = nodes.slice(index + 1, end === -1 ? undefined : end);
                if (!narrows(subject)) {
                    breaks.push({
                        rule: 'unnarrowedSibling', node: subject[0], length: subject.map(String).join('').trim().length,
                        note: `no class, id or attribute after "${node.value}"`,
                    });
                }
            }
            compoundStart = index + 1;
        });
    }
    return breaks;
}

function listStylesheets() {
    const output = execFileSync(
        'git',
        ['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', '*.css'],
        { cwd: repoRoot, encoding: 'utf8' },
    );
    return output.split('\0').filter(file => file && !isVendored(file)).sort();
}

/** Maps a position inside the selector text onto the stylesheet. */
function filePosition(rule, { line, column }) {
    const start = rule.source.start;
    return line === 1 ? { line: start.line, column: start.column + column - 1 } : { line: start.line + line - 1, column };
}

function excerpt(lines, { line, column }, length, note) {
    const text = lines[line - 1] ?? '';
    const indent = text.length - text.trimStart().length;
    const caret = ' '.repeat(Math.max(0, column - 1 - indent)) + '^'.repeat(Math.max(1, length));
    return `      ${text.trimStart()}\n      ${caret} ${note}`;
}

const reports = Object.fromEntries(Object.keys(RULES).map(rule => [rule, []]));
const unparsed = [];
let ruleCount = 0;
const stylesheets = listStylesheets();

for (const file of stylesheets) {
    const source = await fs.readFile(path.join(repoRoot, file), 'utf8');
    let root;
    try {
        root = postcss.parse(source, { from: file });
    } catch (error) {
        unparsed.push(`- ${file}:${error.line}:${error.column}\n      ${error.reason}`);
        continue;
    }
    const lines = source.split(/\r?\n/u);
    root.walkRules(rule => {
        if (rule.parent?.type === 'atrule' && /keyframes$/iu.test(rule.parent.name)) return;
        ruleCount += 1;
        let selector;
        try {
            selector = selectorParser().astSync(blankComments(rule.raws.selector?.raw ?? rule.selector));
        } catch (error) {
            const at = rule.source.start;
            unparsed.push(`- ${file}:${at.line}:${at.column}\n      ${error.message}`);
            return;
        }
        for (const { rule: broken, node, length, note } of findBreaks(selector)) {
            const at = node?.source?.start ? filePosition(rule, node.source.start) : rule.source.start;
            reports[broken].push(`- ${file}:${at.line}:${at.column}\n${excerpt(lines, at, length, note)}`);
        }
    });
}

const breakCount = Object.values(reports).reduce((sum, list) => sum + list.length, 0);

if (unparsed.length > 0) {
    const count = unparsed.length === 1 ? '1 parse error' : `${unparsed.length} parse errors`;
    console.error(`${TAG} FAILED: ${count}; the selectors there were not checked.\n`);
    console.error(`${unparsed.join('\n\n')}\n`);
    process.exitCode = 1;
}
if (breakCount > 0) {
    const count = breakCount === 1 ? '1 selector breaks' : `${breakCount} selectors break`;
    console.error(`${TAG} FAILED: ${count} the CSS performance rules in docs/FrontendGuide.md §6.6.`);
    for (const [rule, list] of Object.entries(reports)) {
        if (list.length === 0) continue;
        const { title, why, fix } = RULES[rule];
        console.error(`\n== ${title} (${list.length})\n`);
        console.error(list.join('\n\n'));
        console.error(`\nWhy: ${why}\nFix:\n${fix.map(line => `  ${line}`).join('\n')}`);
    }
    process.exitCode = 1;
}
if (process.exitCode !== 1) {
    console.log(`${TAG} OK (${stylesheets.length} stylesheets, ${ruleCount} rules)`);
}
