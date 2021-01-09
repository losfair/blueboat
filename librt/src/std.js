import * as workerFetch from "worker-fetch";

class Console {
    constructor() {

    }

    log(text) {
        _callService({
            Sync: {
                Log: "" + text
            }
        });
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
            console.log("respondWith exception: " + e);
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
        _callService({
            Sync: {
                SendFetchResponse: {
                    status: res.status,
                    body: {
                        Binary: Array.from(new Uint8Array(body)),
                    },
                    headers: headers,
                }
            }
        });
        _callService({
            Sync: "Done",
        })
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
        _callService({
            Async: {
                SetTimeout: ms,
            }
        }, callback);
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
    console.log("event type: " + ty);
    switch(ty) {
        case "Fetch": {
            let rawReq = ev[ty].request;

            let headers = new Headers(
                Object.keys(rawReq.headers)
                    .map(k => rawReq.headers[k].map(v => [k, v]))
                    .flat()
            );
            
            let body = null;
            if(rawReq.body) {
                if(rawReq.body.Text) {
                    body = rawReq.body.Text;
                } else {
                    body = new Uint8Array(rawReq.body.Binary).buffer;
                }
            }

            let req = new workerFetch.Request(rawReq.url, {
                method: rawReq.method,
                headers: headers,
                body: body,
            });
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
    return _callService({
        Sync: {
            GetFile: name,
        }
    });
}

export const crypto = {
    getRandomValues(n) {
        return _callService({
            Sync: {
                GetRandomValues: n,
            }
        });
    }
};

class KvNamespace {
    /**
     * 
     * @param {string} name 
     */
    constructor(name) {
        this.name = name;
    }

    /**
     * @param {ArrayBuffer | ArrayLike<number>} key
     * @returns {Promise<ArrayBuffer>}
     */
    getRaw(key) {
        return new Promise((resolve, reject) => {
            _callService({
                Async: {
                    KvGet: {
                        namespace: this.name,
                        key: Array.from(new Uint8Array(key)),
                    }
                }
            }, (result) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    if(result.Ok.Ok !== null) {
                        resolve(new Uint8Array(result.Ok.Ok).buffer);
                    } else {
                        resolve(null);
                    }
                }
            })
        });
    }

    /**
     * @param {string} key
     * @returns {Promise<string>}
     */
    async get(key) {
        let keyRaw = new TextEncoder().encode(key);
        let buf = await this.getRaw(keyRaw);
        if(buf !== null) {
            return new TextDecoder().decode(buf);
        } else {
            return null;
        }
    }

    /**
     * @param {ArrayBuffer | ArrayLike<number>} key
     * @param {ArrayBuffer | ArrayLike<number>} value
     * @returns {Promise<void>}
     */
    putRaw(key, value) {
        return new Promise((resolve, reject) => {
            _callService({
                Async: {
                    KvPut: {
                        namespace: this.name,
                        key: Array.from(new Uint8Array(key)),
                        value: Array.from(new Uint8Array(value)),
                    }
                }
            }, (result) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    resolve();
                }
            })
        });
    }

    /**
     * @param {string} key
     * @param {string} value
     * @returns {Promise<void>}
     */
    async put(key, value) {
        let keyRaw = new TextEncoder().encode(key);
        let valueRaw = new TextEncoder().encode(value);
        await this.putRaw(keyRaw, valueRaw);
    }
}

const kvHandler = {
    get: function(target, prop, receiver) {
        return new KvNamespace(prop);
    }
}

export const kv = new Proxy({}, kvHandler);

export const console = new Console();
export const Request = workerFetch.Request;
export const Response = workerFetch.Response;
export const Headers = workerFetch.Headers;
export const fetch = workerFetch.fetch;
