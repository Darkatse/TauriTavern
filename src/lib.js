/**
 * Library Module for TauriTavern
 *
 * This module provides access to all the libraries used by SillyTavern.
 * It gets the libraries from the global scope (window object) where they were
 * exposed by lib-bundle.js and loaded by lib-loader.js.
 */

// Check if we're running in a Tauri environment
const isTauri = typeof window !== 'undefined' && window.__TAURI__ !== undefined;

// Log initialization
console.log('lib.js: Initializing libraries, Tauri environment:', isTauri);

// Wait for the library loader to complete if it's running
if (window.__TAURITAVERN_LIB_LOADER_RUNNING__) {
    console.log('lib.js: Waiting for library loader to complete...');
}

// Get the libraries from the global scope
// These are loaded by lib-loader.js and exposed by lib-bundle.js
const lodash = window.lodash;
const Fuse = window.Fuse;
const DOMPurify = window.DOMPurify;
const hljs = window.hljs;
const localforage = window.localforage;
const Handlebars = window.Handlebars;
const css = window.css;
const Bowser = window.Bowser;
const DiffMatchPatch = window.DiffMatchPatch;
const isProbablyReaderable = window.isProbablyReaderable;
const Readability = window.Readability;
const SVGInject = window.SVGInject;
const showdown = window.showdown;
const moment = window.moment;
const seedrandom = window.seedrandom;
const Popper = window.Popper;
const droll = window.droll;
const morphdom = window.morphdom;
const slideToggle = window.slideToggle;
const chalk = window.chalk;

// Check if all libraries are available
const missingLibraries = [
    'lodash', 'Fuse', 'DOMPurify', 'hljs', 'localforage', 'Handlebars',
    'css', 'Bowser', 'DiffMatchPatch', 'isProbablyReaderable', 'Readability',
    'SVGInject', 'showdown', 'moment', 'seedrandom', 'Popper', 'droll',
    'morphdom', 'slideToggle', 'chalk'
].filter(lib => eval(lib) === undefined);

if (missingLibraries.length > 0) {
    console.warn('lib.js: Some libraries are not available:', missingLibraries);
} else {
    console.log('lib.js: Successfully accessed all libraries from global scope');
}

/**
 * Create a library bundle object with all libraries
 */
const libraryBundle = {
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
};

/**
 * Expose the libraries to the 'window' object.
 * Needed for compatibility with old extensions.
 */
window.SillyTavernLibs = libraryBundle;

/**
 * Export all libraries
 */
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
};

/**
 * Expose the libraries to the 'window' object.
 * Needed for compatibility with old extensions.
 * Note: New extensions are encouraged to import the libraries directly from lib.js.
 */
export function initLibraryShims() {
    if (!window) {
        return;
    }

    // Expose all libraries to the global scope
    window.lodash = lodash;
    window.Fuse = Fuse;
    window.DOMPurify = DOMPurify;
    window.hljs = hljs;
    window.localforage = localforage;
    window.Handlebars = Handlebars;
    window.css = css;
    window.Bowser = Bowser;
    window.DiffMatchPatch = DiffMatchPatch;
    window.isProbablyReaderable = isProbablyReaderable;
    window.Readability = Readability;
    window.SVGInject = SVGInject;
    window.showdown = showdown;
    window.moment = moment;
    window.seedrandom = seedrandom;
    window.Popper = Popper;
    window.droll = droll;
    window.morphdom = morphdom;
    window.slideToggle = slideToggle;
    window.chalk = chalk;

    // Create global variables for compatibility
    window.__TAURITAVERN_LIB__ = libraryBundle;
    window.__TAURITAVERN_LIB_SHIM__ = libraryBundle;

    console.log('lib.js: Library shims initialized');
}

// Initialize library shims immediately
initLibraryShims();

/**
 * Default export with all libraries
 */
export default libraryBundle;

