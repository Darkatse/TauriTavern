import path from 'path';
import { fileURLToPath } from 'url';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export default {
  mode: 'production',
  entry: {
    lib: './src/lib-bundle.js'
  },
  output: {
    filename: '[name].bundle.js',
    path: path.resolve(__dirname, 'dist'),
    library: {
      type: 'module'
    }
  },
  experiments: {
    outputModule: true,
  },
  module: {
    rules: [
      {
        test: /\.js$/,
        exclude: /node_modules/,
        use: {
          loader: 'babel-loader',
          options: {
            presets: ['@babel/preset-env']
          }
        }
      }
    ]
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
  // Prevent bundling of certain imported packages and instead retrieve these external dependencies at runtime
  externals: {
    // Add any libraries that should be loaded from CDN or global scope
  }
};
