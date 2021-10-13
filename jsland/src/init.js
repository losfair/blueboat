const { TextEncoder, TextDecoder } = require("./text_codec");
const { console } = require("./console");
const crypto = require("./crypto");

globalThis.self = globalThis;
globalThis.TextEncoder = TextEncoder;
globalThis.TextDecoder = TextDecoder;
globalThis.console = console;
globalThis.crypto = crypto;

const { URL, URLSearchParams } = require("whatwg-url");
globalThis.URL = URL;
globalThis.URLSearchParams = URLSearchParams;

require("./fetch");
require("./index");
