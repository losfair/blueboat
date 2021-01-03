global.process = require("process");
global.Buffer = require("buffer").Buffer;
require("fast-text-encoding");
require("./url.js");
require("url-search-params-polyfill");
const std = require("./std.js");

Object.assign(global, std);
