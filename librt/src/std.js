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
        if(res instanceof Promise) {
            res = await res;
        }
        if(!(res instanceof Response)) {
            throw new TypeError("respondWith: expecting a Response");
        }

        let body = await res.text();
        _callService({
            Sync: {
                SendFetchResponse: {
                    status: res.status,
                    body: {
                        Text: body,
                    },
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
            let req = new workerFetch.Request(rawReq.url);
            dispatchEvent(new FetchEvent(req))
            break;
        }
        default: {
            throw new TypeError("bad event type: " + ty);
        }
    }
}

export const console = new Console();
export const Request = workerFetch.Request;
export const Response = workerFetch.Response;
