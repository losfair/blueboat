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
