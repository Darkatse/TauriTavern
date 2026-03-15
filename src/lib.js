/**
 * Library module facade for TauriTavern.
 *
 * We load a small core webpack bundle (`src/dist/lib.core.bundle.js`) and
 * re-export the libraries expected by SillyTavern frontend modules.
 *
 * Heavy / feature-specific libraries live in `src/dist/lib.optional.bundle.js`
 * and are loaded on demand via async helpers.
 */
import libCoreBundle, {
    lodash,
    Fuse,
    DOMPurify,
    localforage,
    Handlebars,
    css,
    Bowser,
    DiffMatchPatch,
    SVGInject,
    showdown,
    moment,
    seedrandom,
    Popper,
    droll,
    morphdom,
    chevrotain,
} from './dist/lib.core.bundle.js';

let optionalBundlePromise = null;
let stscriptLanguageRegistered = false;

async function loadOptionalBundle() {
    if (optionalBundlePromise) {
        return optionalBundlePromise;
    }

    optionalBundlePromise = import('./dist/lib.optional.bundle.js');
    return optionalBundlePromise;
}

export async function getHljs() {
    const { hljs } = await loadOptionalBundle();

    if (!stscriptLanguageRegistered) {
        const { registerStscriptLanguage } = await import('./scripts/slash-commands/stscript-hljs-language.js');
        registerStscriptLanguage(hljs);
        stscriptLanguageRegistered = true;
    }

    if (typeof window !== 'undefined' && !('hljs' in window)) {
        window.hljs = hljs;
    }

    return hljs;
}

export async function getReadability() {
    const { Readability, isProbablyReaderable } = await loadOptionalBundle();
    return { Readability, isProbablyReaderable };
}

/**
 * Expose selected libraries on window for third-party extension compatibility.
 * New code should import from lib.js directly.
 */
export function initLibraryShims() {
    if (typeof window === 'undefined') {
        return;
    }

    if (!('Fuse' in window)) {
        window.Fuse = Fuse;
    }
    if (!('DOMPurify' in window)) {
        window.DOMPurify = DOMPurify;
    }
    if (!('localforage' in window)) {
        window.localforage = localforage;
    }
    if (!('Handlebars' in window)) {
        window.Handlebars = Handlebars;
    }
    if (!('diff_match_patch' in window)) {
        window.diff_match_patch = DiffMatchPatch;
    }
    if (!('SVGInject' in window)) {
        window.SVGInject = SVGInject;
    }
    if (!('showdown' in window)) {
        window.showdown = showdown;
    }
    if (!('moment' in window)) {
        window.moment = moment;
    }
    if (!('Popper' in window)) {
        window.Popper = Popper;
    }
    if (!('droll' in window)) {
        window.droll = droll;
    }
}

export {
    lodash,
    Fuse,
    DOMPurify,
    localforage,
    Handlebars,
    css,
    Bowser,
    DiffMatchPatch,
    SVGInject,
    showdown,
    moment,
    seedrandom,
    Popper,
    droll,
    morphdom,
    chevrotain,
};

export default libCoreBundle;
