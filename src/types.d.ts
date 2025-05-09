// Type declarations for modules without type definitions

declare module 'crypto-browserify';
declare module 'stream-browserify';
declare module 'os-browserify/browser';
declare module 'slidetoggle';
declare module 'droll';
declare module '@iconfu/svg-inject';

// Global variables
interface Window {
    // Tauri globals
    __TAURI__?: any;
    __TAURI_INTERNALS__?: any;
    __TAURI_RUNNING__?: boolean;

    // TauriTavern globals
    __TAURITAVERN_LIB_BUNDLE_LOADED__?: boolean;
    __TAURITAVERN_LIB_SHIM_LOADED__?: boolean;
    __TAURITAVERN_LIB__?: any;
    __TAURITAVERN_LIB_SHIM__?: any;
    __TAURITAVERN_LIB_LOADER_RUNNING__?: boolean;
    __TAURITAVERN_LIB_LOADER_COMPLETED__?: boolean;
    SillyTavernLibs?: any;

    // Library globals
    lodash?: any;
    Fuse?: any;
    DOMPurify?: any;
    hljs?: any;
    localforage?: any;
    Handlebars?: any;
    css?: any;
    Bowser?: any;
    DiffMatchPatch?: any;
    isProbablyReaderable?: any;
    Readability?: any;
    SVGInject?: any;
    showdown?: any;
    moment?: any;
    seedrandom?: any;
    Popper?: any;
    droll?: any;
    morphdom?: any;
    slideToggle?: any;
    chalk?: any;
}
