// Build the real UI tools for browser/WebView verification; serve the repository root afterwards.
// Open /.cache/app-use-smoke/index.html and run the checks.
import path from 'node:path';
import { rspack } from '@rspack/core';
import { createRspackConfigs } from '../../rspack.config.js';

const config = createRspackConfigs('development').find(config => config.name === 'in-app-agent');
config.entry = { test: './tests/browser/app-use-iframe.js' };
config.output = { ...config.output, path: path.resolve('.cache/app-use-smoke'), clean: true };
config.plugins = [...(config.plugins ?? []), new rspack.HtmlRspackPlugin({
    title: 'App Use iframe checks',
    scriptLoading: 'module',
    templateContent: '<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>App Use iframe checks</title><button id="run">Run iframe checks</button><pre id="result">Ready</pre><div id="fixture"></div>',
})];
const compiler = rspack(config);
compiler.run((error, stats) => {
    if (error || stats.hasErrors()) {
        console.error(error || stats.toString({ all: false, errors: true }));
        process.exitCode = 1;
    } else console.log('Open /.cache/app-use-smoke/index.html through a local HTTP server.');
    compiler.close(() => {});
});
