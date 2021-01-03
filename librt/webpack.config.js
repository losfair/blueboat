module.exports = {
    mode: "production",
    optimization: {
        minimize: false,
    },
    resolve: {
        fallback: {
            url: require.resolve("url/"),
            util: require.resolve("util/"),
            stream: require.resolve("stream-browserify"),
            zlib: require.resolve("browserify-zlib"),
            buffer: require.resolve("buffer/"),
            assert: require.resolve("assert/"),
            process: require.resolve("process/"),
        }
    }
};
