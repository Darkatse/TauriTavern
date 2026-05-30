// Core library bundle for TauriTavern.
//
// Keep this file limited to libraries required during Shell/Core startup stages.
// Heavy / feature-specific libraries should live in lib-bundle-optional.js and be
// loaded via `lib.js` on demand.

import lodash from 'lodash';
import Fuse from 'fuse.js';
import DOMPurify from 'dompurify';
import localforage from 'localforage';
import Handlebars from 'handlebars';
import css from '@adobe/css-tools';
import Bowser from 'bowser';
import DiffMatchPatch from 'diff-match-patch';
import SVGInject from '@iconfu/svg-inject';
import showdown from 'showdown';
import moment from 'moment';
import seedrandom from 'seedrandom';
import * as Popper from '@popperjs/core';
import droll from 'droll';
import morphdom from 'morphdom';
import * as chevrotain from 'chevrotain';
import { gzipSync, gzip } from 'fflate';
import jsSha256Module from 'js-sha256';

const jsSha256 = jsSha256Module.sha256 ?? jsSha256Module;

const libBundle = {
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
    gzipSync,
    gzip,
    sha256: jsSha256,
    initialized: true,
};

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
    gzipSync,
    gzip,
    jsSha256 as sha256,
};

export default libBundle;
