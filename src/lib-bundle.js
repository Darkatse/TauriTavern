// This file bundles all the libraries used by SillyTavern
// It's used by webpack to create a single bundle file

// Import all the libraries
import lodash from 'lodash';
import Fuse from 'fuse.js';
import DOMPurify from 'dompurify';
import hljs from 'highlight.js';
import localforage from 'localforage';
import Handlebars from 'handlebars';
import * as css from '@adobe/css-tools';
import Bowser from 'bowser';
import DiffMatchPatch from 'diff-match-patch';
import { isProbablyReaderable, Readability } from '@mozilla/readability';
import SVGInject from '@iconfu/svg-inject';
import showdown from 'showdown';
import moment from 'moment';
import seedrandom from 'seedrandom';
import * as Popper from '@popperjs/core';
import droll from 'droll';
import morphdom from 'morphdom';
import { toggle as slideToggle } from 'slidetoggle';
import chalk from 'chalk';

// Create a bundle object with all the libraries
const libBundle = {
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
    initialized: true
};

// Export all the libraries
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
    chalk
};

// Export the bundle object
export default libBundle;

// Log a message to the console
console.log('TauriTavern Library Bundle loaded');

// Set a global variable to indicate that the bundle is loaded
window.__TAURITAVERN_LIB_BUNDLE_LOADED__ = true;

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

console.log('All libraries exposed to global scope');
