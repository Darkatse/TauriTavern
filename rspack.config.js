import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as rspack from '@rspack/core';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const cacheBuildDependencies = [
  'rspack.config.js',
  'package.json',
  'pnpm-lock.yaml',
].map(resolveRepoPath);

function resolveRepoPath(file) {
  return path.resolve(__dirname, file);
}

function createCache(mode, name) {
  if (mode === 'development') {
    return { type: 'memory' };
  }

  return {
    type: 'persistent',
    buildDependencies: cacheBuildDependencies,
    storage: {
      type: 'filesystem',
      directory: path.resolve(__dirname, '.cache/rspack', name),
    },
  };
}

const sharedResolve = {
  extensions: ['.tsx', '.ts', '.js'],
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

function createReactModule(development) {
  return {
    rules: [
      {
        test: /\.tsx?$/,
        exclude: /node_modules/,
        type: 'javascript/auto',
        use: {
          loader: 'builtin:swc-loader',
          options: {
            detectSyntax: 'auto',
            jsc: {
              transform: {
                react: {
                  development,
                  runtime: 'automatic',
                },
              },
            },
          },
        },
      },
    ],
  };
}

function createSharedConfig(mode, name) {
  return {
    name,
    mode,
    bail: mode === 'production',
    target: ['web', 'es2020'],
    cache: createCache(mode, name),
    resolve: sharedResolve,
    optimization: sharedOptimization,
    performance: sharedPerformance,
    stats: sharedStats,
  };
}

export function createRspackConfigs(mode = 'production') {
  const development = mode === 'development';

  const coreConfig = {
    ...createSharedConfig(mode, 'vendor-libs'),
    entry: {
      'lib.core': './src/lib-bundle-core.js',
      'lib.optional': './src/lib-bundle-optional.js',
    },
    output: {
      filename: '[name].bundle.js',
      path: path.resolve(__dirname, 'src/dist'),
      module: true,
      clean: true,
      library: {
        type: 'module'
      }
    },
  };

  const agentSystemConfig = {
    ...createSharedConfig(mode, 'agent-system'),
    dependencies: ['vendor-libs'],
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
    module: createReactModule(development),
    plugins: [
      createVueDefinePlugin(),
    ],
  };

  const mcpManagerConfig = {
    ...createSharedConfig(mode, 'mcp-manager'),
    entry: {
      index: './src/scripts/extensions/mcp-manager/src/index.tsx',
    },
    output: {
      filename: '[name].bundle.js',
      path: path.resolve(__dirname, 'src/scripts/extensions/mcp-manager/dist'),
      module: true,
      library: {
        type: 'module'
      },
      clean: true,
    },
    module: createReactModule(development),
  };

  const tauriTavernSettingsConfig = {
    ...createSharedConfig(mode, 'tauritavern-settings'),
    dependencies: ['vendor-libs'],
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
    module: createReactModule(development),
    plugins: [
      createVueDefinePlugin(),
    ],
  };

  return [coreConfig, agentSystemConfig, mcpManagerConfig, tauriTavernSettingsConfig];
}

export default (_env, argv = {}) => createRspackConfigs(argv.mode ?? 'production');
