// Core library bundle for TauriTavern.
//
// Keep the synchronous /lib.js ABI here. Libraries with an intentionally async
// API should live in lib-bundle-optional.js and be loaded via `lib.js` on demand.

import lodash from 'lodash';
import Fuse from 'fuse.js';
import DOMPurify from 'dompurify';
import localforage from 'localforage';
import Handlebars from 'handlebars';
import css from '@adobe/css-tools';
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
import chalk from 'chalk';
import yaml from 'yaml';
import * as chevrotain from 'chevrotain';
import { gzipSync, gzip } from 'fflate';
import jsSha256Module from 'js-sha256';
import {
    Virtualizer,
    defaultRangeExtractor,
    observeElementOffset,
    observeElementRect,
} from '@tanstack/virtual-core';

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
    Readability,
    isProbablyReaderable,
    SVGInject,
    showdown,
    moment,
    seedrandom,
    Popper,
    droll,
    morphdom,
    chalk,
    yaml,
    chevrotain,
    gzipSync,
    gzip,
    sha256: jsSha256,
    Virtualizer,
    defaultRangeExtractor,
    observeElementOffset,
    observeElementRect,
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
    Readability,
    isProbablyReaderable,
    SVGInject,
    showdown,
    moment,
    seedrandom,
    Popper,
    droll,
    morphdom,
    chalk,
    yaml,
    chevrotain,
    gzipSync,
    gzip,
    jsSha256 as sha256,
    Virtualizer,
    defaultRangeExtractor,
    observeElementOffset,
    observeElementRect,
};

export default libBundle;
