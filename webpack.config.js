import path from 'path';
import { fileURLToPath } from 'url';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default {
  mode: 'production',
  target: ['web', 'es2020'],
  cache: {
    type: 'filesystem',
    cacheDirectory: path.resolve(__dirname, '.cache/webpack'),
  },
  entry: {
    lib: './src/lib-bundle.js'
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
  resolve: {
    extensions: ['.js'],
    fallback: {
      "path": false,
      "fs": false,
      "crypto": false,
      "stream": false,
      "buffer": false,
      "util": false,
      "assert": false,
      "os": false
    }
  },
  optimization: {
    moduleIds: 'deterministic',
    chunkIds: 'deterministic',
  },
};
