/**
 * Library module facade for TauriTavern.
 *
 * We load a single webpack bundle (`src/dist/lib.bundle.js`) and re-export
 * the libraries expected by SillyTavern frontend modules.
 */
import libBundle, {
    lodash,
    Fuse,
    DOMPurify,
    hljs,
    localforage,
    Handlebars,
    css,
    Bowser,
    DiffMatchPatch,
    isProbablyReaderable,
    Readability,
    SVGInject,
    showdown,
    moment,
    seedrandom,
    Popper,
    droll,
    morphdom,
    slideToggle,
    chalk,
    yaml,
    chevrotain,
    moduleLexerInit,
    moduleLexerParse,
} from './dist/lib.bundle.js';

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
    if (!('hljs' in window)) {
        window.hljs = hljs;
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
    hljs,
    localforage,
    Handlebars,
    css,
    Bowser,
    DiffMatchPatch,
    Readability,
    isProbablyReaderable,
    SVGInject,
    showdown,
    moment,
    seedrandom,
    Popper,
    droll,
    morphdom,
    slideToggle,
    chalk,
    yaml,
    chevrotain,
    moduleLexerInit,
    moduleLexerParse,
};

export default libBundle;
