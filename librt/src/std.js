import * as workerFetch from "worker-fetch";

class Console {
    constructor() {

    }

    log(text) {
        _callServiceWrapper({
            Sync: {
                Log: "" + text
            }
        }, []);
    }
}

class FetchEvent {
    /**
     * 
     * @param {string} type 
     * @param {Object} request 
     */
    constructor(request) {
        this.type = "fetch";
        this.request = request;
    }

    async respondWith(res) {
        try {
            await this._respondWith(res);
        } catch(e) {
            console.log("caught exception in respondWith");
            if(e && e.stack) console.log(e.stack);
            else console.log(e);
            await this._respondWith(new Response("caught exception when handling request", { status: 500 }));
        }
    }

    async _respondWith(res) {
        if(res instanceof Promise) {
            res = await res;
        }
        if(!(res instanceof Response)) {
            throw new TypeError("respondWith: expecting a Response");
        }

        /**
         * @type {Object.<string, string[]>}
         */
        let headers = {};

        for(let pair of res.headers.entries()) {
            let k = pair[0];
            let v = pair[1];
            if(!headers[k]) headers[k] = [];
            headers[k].push(v);
        }

        let body = await res.arrayBuffer();
        _callServiceWrapper({
            Sync: {
                SendFetchResponse: {
                    status: res.status,
                    headers: headers,
                }
            }
        }, [body]);
        _callServiceWrapper({
            Sync: "Done",
        }, [])
    }
}

/**
 * @type {Object.<string, Object[]>}
 */
let eventListeners = {};

export function addEventListener(eventName, listener) {
    if(!eventListeners[eventName]) {
        eventListeners[eventName] = [];
    }

    let obj;
    if(typeof(listener) == "function") {
        obj = {
            handleEvent: listener
        };
    } else {
        obj = listener;
    }

    eventListeners[eventName].push(obj);
}

export function dispatchEvent(event) {
    let listeners = eventListeners[event.type];
    if(listeners) {
        for(let l of listeners) {
            l.handleEvent(event);
        }
    }
}

/**
 * @type {Map<number, boolean>}
 */
let inflightTimeouts = new Map();
let nextTimeoutId = 1;

/**
 * 
 * @param {function} callback 
 * @param {number} ms 
 * @param {any[]} args 
 * @param {boolean} isInterval 
 * @returns {number}
 */
function scheduleTimeoutOrInterval(callback, ms, args, isInterval) {
    let id = nextTimeoutId;
    nextTimeoutId++;
    inflightTimeouts.set(id, isInterval);

    function onFire() {
        if(inflightTimeouts.has(id)) {
            let isInterval = inflightTimeouts.get(id);
            if(isInterval) {
                schedule(ms, onFire);
            } else {
                inflightTimeouts.delete(id);
            }
            callback.call(this, args);
        }
    }

    function schedule(ms, callback) {
        _callServiceWrapper({
            Async: {
                SetTimeout: ms,
            }
        }, [], callback);
    }

    schedule(ms, onFire);
    return id;
}

/**
 * 
 * @param {function} callback 
 * @param {number} ms 
 * @param  {...any} args 
 * @returns {number}
 */
export function setTimeout(callback, ms, ...args) {
    return scheduleTimeoutOrInterval(callback, ms, args, false);
}
/**
 * 
 * @param {function} callback 
 * @param {number} ms 
 * @param  {...any} args 
 * @returns {number}
 */
export function setInterval(callback, ms, ...args) {
    return scheduleTimeoutOrInterval(callback, ms, args, true);
}

/**
 * 
 * @param {number} id 
 */
export function clearTimeout(id) {
    inflightTimeouts.delete(id);
}

/**
 * 
 * @param {number} id 
 */
export function clearInterval(id) {
    inflightTimeouts.delete(id);
}

/**
 * 
 * @param {Object} ev 
 */
export function _dispatchEvent(ev) {
    let ty = Object.keys(ev)[0];
    switch(ty) {
        case "Fetch": {
            let rawReq = ev[ty].request;

            let headers = new Headers(
                Object.keys(rawReq.headers)
                    .map(k => rawReq.headers[k].map(v => [k, v]))
                    .flat()
            );
            
            let body = null;
            if(rawReq.body && rawReq.body.Binary.length) {
                body = new Uint8Array(rawReq.body.Binary).buffer;
            }

            let req = new workerFetch.Request(rawReq.url, {
                method: rawReq.method,
                headers: headers,
                body: body,
            });
            //console.log(`[request] ${req.method} ${req.url} x-forwarded-for(${req.headers.get("x-forwarded-for")})`);
            let targetEvent = new FetchEvent(req);
            try {
                dispatchEvent(targetEvent);
            } catch(e) {
                console.log("dispatchEvent exception: " + e);
                targetEvent._respondWith(new Response("caught exception when dispatching request", { status: 500 }));
            }
            break;
        }
        default: {
            throw new TypeError("bad event type: " + ty);
        }
    }
}

export function getFileFromBundle(name) {
    return _callServiceWrapper({
        Sync: {
            GetFile: name,
        }
    }, []);
}

export const crypto = {
    getRandomValues(bufferOrView) {
        return _callServiceWrapper({
            Sync: "GetRandomValues",
        }, [bufferOrView]);
    },
    subtle: require("./subtle_crypto.js"),
};

export const kv = require("./kv.js").kv;

export const console = new Console();
export const Request = workerFetch.Request;
export const Response = workerFetch.Response;
export const Headers = workerFetch.Headers;
export const fetch = workerFetch.fetch;

export function _callServiceWrapper(cmd, buffers, cb) {
    let serialized = JSON.stringify(cmd);
    return _callService(serialized, buffers, cb);
}

export function _callService(cmd, buffers, cb) {
    let wrappedCb = function(data, ...args) {
        return cb(JSON.parse(new TextDecoder().decode(data)), ...args);
    };
    return _rt_callService(cmd, buffers, cb ? wrappedCb : null);
}
