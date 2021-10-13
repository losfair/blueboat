const path = require('path');
const webpack = require("webpack");

module.exports = {
  entry: './src/init.js',
  mode: 'production',
  module: {
    rules: [
      {
        test: /\.ts$/,
        use: 'ts-loader',
        exclude: /node_modules/,
      },
    ],
  },
  resolve: {
    extensions: ['.ts', '.js'],
  },
  output: {
    filename: 'bundle.js',
    path: path.resolve(__dirname, 'dist'),
  },
  optimization: {
    minimize: false,
  },
  plugins: [
    new webpack.DefinePlugin({
      SharedArrayBuffer: {
        prototype: {
          byteLength: {}
        }
      }
    }),
  ]
};