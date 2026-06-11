import crypto from 'crypto';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import * as rspack from '@rspack/core';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const cacheEnvironment = `${process.platform}-${process.arch}-node${process.versions.node.split('.')[0]}`;

const commonCacheInputs = [
  'rspack.config.js',
  'package.json',
  'pnpm-lock.yaml',
];

const libraryCacheInputs = [
  ...commonCacheInputs,
  'src/lib.js',
  'src/lib-bundle-core.js',
  'src/lib-bundle-optional.js',
];

const agentSystemCacheInputs = [
  ...commonCacheInputs,
  'src/scripts/extensions/agent-system/src/index.js',
  'src/scripts/tauritavern/agent/agent-run-controller.js',
  'src/scripts/tauritavern/agent/agent-run-retry.js',
];

const tauriSettingUiCacheInputs = [
  ...commonCacheInputs,
  ...listJavaScriptFiles('src/scripts/tauri/setting/settings-app'),
  ...listJavaScriptFiles('src/scripts/tauri/setting/dev-logs-app'),
  ...listJavaScriptFiles('src/scripts/tauri/setting/sync-app'),
];

function resolveRepoPath(file) {
  return path.resolve(__dirname, file);
}

function listJavaScriptFiles(relativeDir) {
  const root = resolveRepoPath(relativeDir);
  const results = [];
  const stack = [root];

  while (stack.length > 0) {
    const current = stack.pop();
    const entries = fs.readdirSync(current, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
        continue;
      }

      if (entry.isFile() && path.extname(entry.name) === '.js') {
        results.push(path.relative(__dirname, fullPath).replace(/\\/g, '/'));
      }
    }
  }

  return results.sort();
}

function buildCacheVersion(name, inputFiles) {
  const hash = crypto.createHash('sha256');
  hash.update(`name=${name}\n`);
  hash.update(`platform=${process.platform}\n`);
  hash.update(`arch=${process.arch}\n`);
  hash.update(`node=${process.versions.node}\n`);
  hash.update(`rspack=${rspack.rspackVersion}\n`);

  for (const file of inputFiles) {
    hash.update(`file=${file}\n`);
    hash.update(fs.readFileSync(resolveRepoPath(file)));
    hash.update('\n');
  }

  return hash.digest('hex');
}

function createPersistentCache(name, inputFiles) {
  return {
    type: 'persistent',
    version: buildCacheVersion(name, inputFiles),
    buildDependencies: commonCacheInputs.map(resolveRepoPath),
    storage: {
      type: 'filesystem',
      directory: path.resolve(__dirname, '.cache/rspack', cacheEnvironment, name),
    },
  };
}

const sharedResolve = {
  extensions: ['.js'],
  alias: {
    '/lib.js': path.resolve(__dirname, 'src/lib.js'),
    '/script.js': path.resolve(__dirname, 'src/script.js'),
    '/scripts': path.resolve(__dirname, 'src/scripts'),
  },
  fallback: {
    "path": false,
    "fs": false,
    "crypto": false,
    "stream": false,
    "buffer": false,
    "util": false,
    "assert": false,
    "os": false,
    "http": false,
    "https": false,
    "url": false
  }
};

const sharedOptimization = {
  moduleIds: 'deterministic',
  chunkIds: 'deterministic',
};

const sharedPerformance = {
  hints: false,
  maxEntrypointSize: 5120000,
  maxAssetSize: 5120000
};

const sharedStats = {
  preset: 'normal',
  assets: true,
  chunks: true,
  modules: true,
  entrypoints: true,
  timings: true,
  builtAt: true,
  logging: 'warn',
  cachedAssets: false,
  cachedModules: false,
  chunkModules: false,
  assetsSort: '!size',
  modulesSort: '!size',
  assetsSpace: 20,
  modulesSpace: 20,
};

function createVueDefinePlugin() {
  return new rspack.DefinePlugin({
    __VUE_OPTIONS_API__: JSON.stringify(true),
    __VUE_PROD_DEVTOOLS__: JSON.stringify(false),
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: JSON.stringify(false),
  });
}

const coreConfig = {
  name: 'vendor-libs',
  mode: 'production',
  bail: true,
  target: ['web', 'es2020'],
  cache: createPersistentCache('vendor-libs', libraryCacheInputs),
  entry: {
    'lib.core': './src/lib-bundle-core.js',
    'lib.optional': './src/lib-bundle-optional.js',
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'src/dist'),
    module: true,
    library: {
      type: 'module'
    }
  },
  resolve: sharedResolve,
  optimization: sharedOptimization,
  performance: sharedPerformance,
  stats: sharedStats,
};

const agentSystemConfig = {
  name: 'agent-system',
  dependencies: ['vendor-libs'],
  mode: 'production',
  bail: true,
  target: ['web', 'es2020'],
  cache: createPersistentCache('agent-system', agentSystemCacheInputs),
  entry: {
    index: './src/scripts/extensions/agent-system/src/index.js',
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'src/scripts/extensions/agent-system/dist'),
    module: true,
    library: {
      type: 'module'
    },
    clean: true,
  },
  resolve: sharedResolve,
  optimization: sharedOptimization,
  performance: sharedPerformance,
  stats: sharedStats,
  plugins: [
    createVueDefinePlugin(),
  ],
};

const tauriTavernSettingsConfig = {
  name: 'tauritavern-settings',
  dependencies: ['vendor-libs'],
  mode: 'production',
  bail: true,
  target: ['web', 'es2020'],
  cache: createPersistentCache('tauritavern-settings', tauriSettingUiCacheInputs),
  entry: {
    settings: './src/scripts/tauri/setting/settings-app/index.js',
    'dev-logs': './src/scripts/tauri/setting/dev-logs-app/index.js',
    sync: './src/scripts/tauri/setting/sync-app/index.js',
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'src/scripts/tauri/setting/dist'),
    module: true,
    library: {
      type: 'module'
    },
    clean: true,
  },
  resolve: sharedResolve,
  optimization: sharedOptimization,
  performance: sharedPerformance,
  stats: sharedStats,
  plugins: [
    createVueDefinePlugin(),
  ],
};

export default [coreConfig, agentSystemConfig, tauriTavernSettingsConfig];
