import path from 'path';
import { fileURLToPath } from 'url';
import webpack from 'webpack';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

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

const coreConfig = {
  mode: 'production',
  target: ['web', 'es2020'],
  cache: {
    type: 'filesystem',
    cacheDirectory: path.resolve(__dirname, '.cache/webpack'),
  },
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
  mode: 'production',
  target: ['web', 'es2020'],
  cache: {
    type: 'filesystem',
    cacheDirectory: path.resolve(__dirname, '.cache/webpack-agent-system'),
  },
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
    new webpack.DefinePlugin({
      __VUE_OPTIONS_API__: JSON.stringify(true),
      __VUE_PROD_DEVTOOLS__: JSON.stringify(false),
      __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: JSON.stringify(false),
    }),
  ],
};

export default [coreConfig, agentSystemConfig];
