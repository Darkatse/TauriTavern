import crypto from 'crypto';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import webpack from 'webpack';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const cacheEnvironment = `${process.platform}-${process.arch}-node${process.versions.node.split('.')[0]}`;

const commonCacheInputs = [
  'webpack.config.js',
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
  hash.update(`webpack=${webpack.version}\n`);

  for (const file of inputFiles) {
    hash.update(`file=${file}\n`);
    hash.update(fs.readFileSync(resolveRepoPath(file)));
    hash.update('\n');
  }

  return hash.digest('hex');
}

function createFilesystemCache(name, inputFiles) {
  return {
    type: 'filesystem',
    name,
    cacheDirectory: path.resolve(__dirname, '.cache/webpack', cacheEnvironment, name),
    version: buildCacheVersion(name, inputFiles),
    buildDependencies: {
      config: [__filename],
      inputs: inputFiles.map(resolveRepoPath),
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

function createVueDefinePlugin() {
  return new webpack.DefinePlugin({
    __VUE_OPTIONS_API__: JSON.stringify(true),
    __VUE_PROD_DEVTOOLS__: JSON.stringify(false),
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: JSON.stringify(false),
  });
}

const coreConfig = {
  name: 'vendor-libs',
  mode: 'production',
  target: ['web', 'es2020'],
  cache: createFilesystemCache('vendor-libs', libraryCacheInputs),
  entry: {
    'lib.core': './src/lib-bundle-core.js',
    'lib.optional': './src/lib-bundle-optional.js',
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'src/dist'),
    library: {
      type: 'module'
    }
  },
  experiments: {
    outputModule: true,
  },
  resolve: sharedResolve,
  optimization: sharedOptimization,
  performance: sharedPerformance,
};

const agentSystemConfig = {
  name: 'agent-system',
  dependencies: ['vendor-libs'],
  mode: 'production',
  target: ['web', 'es2020'],
  cache: createFilesystemCache('agent-system', agentSystemCacheInputs),
  entry: {
    index: './src/scripts/extensions/agent-system/src/index.js',
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'src/scripts/extensions/agent-system/dist'),
    library: {
      type: 'module'
    },
    clean: true,
  },
  experiments: {
    outputModule: true,
  },
  resolve: sharedResolve,
  optimization: sharedOptimization,
  performance: sharedPerformance,
  plugins: [
    createVueDefinePlugin(),
  ],
};

const tauriTavernSettingsConfig = {
  name: 'tauritavern-settings',
  dependencies: ['vendor-libs'],
  mode: 'production',
  target: ['web', 'es2020'],
  cache: createFilesystemCache('tauritavern-settings', tauriSettingUiCacheInputs),
  entry: {
    settings: './src/scripts/tauri/setting/settings-app/index.js',
    'dev-logs': './src/scripts/tauri/setting/dev-logs-app/index.js',
    sync: './src/scripts/tauri/setting/sync-app/index.js',
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'src/scripts/tauri/setting/dist'),
    library: {
      type: 'module'
    },
    clean: true,
  },
  experiments: {
    outputModule: true,
  },
  resolve: sharedResolve,
  optimization: sharedOptimization,
  performance: sharedPerformance,
  plugins: [
    createVueDefinePlugin(),
  ],
};

export default [coreConfig, agentSystemConfig, tauriTavernSettingsConfig];
