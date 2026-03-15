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
 * SillyTavern compatibility: slide-toggle animation helper used by some third-party extensions.
 *
 * @param {HTMLElement} el
 * @param {{ miliseconds?: number, transitionFunction?: string, onAnimationEnd?: ((el: HTMLElement) => void) }} [options]
 */
export function slideToggle(el, options = {}) {
    if (!(el instanceof HTMLElement)) {
        throw new Error('slideToggle: expected HTMLElement');
    }

    const duration = Number(options.miliseconds ?? 0) || 0;
    const easing = String(options.transitionFunction ?? 'ease-in-out');
    const onAnimationEnd = options.onAnimationEnd ?? null;

    const $ = globalThis.jQuery || globalThis.$;
    if (typeof $ !== 'function') {
        throw new Error('slideToggle: jQuery is not available');
    }

    const $el = $(el);
    if (typeof $el.transition !== 'function') {
        $el.stop(true, true).slideToggle(duration, () => onAnimationEnd?.(el));
        return;
    }

    const style = getComputedStyle(el);
    const isHidden = style.display === 'none' || el.getBoundingClientRect().height === 0;

    if (isHidden) {
        const display = style.display !== 'none'
            ? style.display
            : (el.classList.contains('fillLeft') || el.classList.contains('fillRight') ? 'flex' : 'block');
        $el.css({ display, overflow: 'hidden', height: 0 });
        const targetHeight = el.scrollHeight || el.getBoundingClientRect().height;
        $el.transition({
            height: targetHeight,
            duration,
            easing,
            complete: () => {
                $el.css({ height: '', overflow: '' });
                onAnimationEnd?.(el);
            },
        });
        return;
    }

    const startHeight = el.getBoundingClientRect().height;
    $el.css({ overflow: 'hidden', height: startHeight });
    $el.transition({
        height: 0,
        duration,
        easing,
        complete: () => {
            $el.css({ display: 'none', height: '', overflow: '' });
            onAnimationEnd?.(el);
        },
    });
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
