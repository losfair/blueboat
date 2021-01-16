

class KvNamespace {
    /**
     * 
     * @param {string} name 
     */
    constructor(name) {
        this.name = name;
    }

    /**
     * @param {ArrayBuffer | ArrayBufferView} key
     * @param {boolean} lock
     * @returns {Promise<ArrayBuffer>}
     */
    getRaw(key, lock = false) {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: {
                    KvGet: {
                        namespace: this.name,
                        lock: lock,
                    }
                }
            }, [key], (result, buffers) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    if(result.Ok.Ok) {
                        resolve(buffers[0]);
                    } else {
                        resolve(null);
                    }
                }
            })
        });
    }

    /**
     * @param {string} key
     * @param {boolean} lock
     * @returns {Promise<string>}
     */
    async get(key, lock = false) {
        let keyRaw = new TextEncoder().encode(key);
        let buf = await this.getRaw(keyRaw.buffer, lock);
        if(buf !== null) {
            return new TextDecoder().decode(buf);
        } else {
            return null;
        }
    }

    /**
     * @param {ArrayBuffer | ArrayBufferView} key
     * @param {ArrayBuffer | ArrayBufferView} value
     * @returns {Promise<void>}
     */
    putRaw(key, value) {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: {
                    KvPut: {
                        namespace: this.name,
                    }
                }
            }, [key, value], (result) => {
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
        await this.putRaw(keyRaw.buffer, valueRaw.buffer);
    }

    /**
     * @param {ArrayBuffer | ArrayBufferView} key
     * @returns {Promise<void>}
     */
    deleteRaw(key) {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: {
                    KvDelete: {
                        namespace: this.name,
                    }
                }
            }, [key], (result) => {
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
     * @returns {Promise<void>}
     */
    async delete(key) {
        let keyRaw = new TextEncoder().encode(key);
        await this.deleteRaw(keyRaw.buffer);
    }

    /**
     * 
     * @param {Object} args 
     * @param {ArrayBuffer | ArrayBufferView} args.start
     * @param {ArrayBuffer | ArrayBufferView} args.startExclusive
     * @param {ArrayBuffer | ArrayBufferView} args.end
     * @param {number} args.limit
     * @param {boolean} args.lock
     * @returns {Promise<ArrayBuffer[]>}
     */
    scanRaw({start, startExclusive = null, end = null, limit = 1, lock = false}) {
        // Append a zero byte to startExclusive, to indicate the immediate next key.
        if(startExclusive !== null) {
            start = new ArrayBuffer(startExclusive.byteLength + 1);
            new Uint8Array(start).set(new Uint8Array(startExclusive));
        }
        let args = end === null ? [start] : [start, end];
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: {
                    KvScan: {
                        namespace: this.name,
                        limit: limit,
                        lock: lock,
                    }
                }
            }, args, (result, buffers) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    resolve(buffers);
                }
            })
        });
    }

    /**
     * 
     * @param {Object} args 
     * @param {string} args.start
     * @param {string} args.startExclusive
     * @param {string} args.end
     * @param {number} args.limit
     * @param {boolean} args.lock
     * @returns {Promise<string[]>}
     */
    async scan({start = "", startExclusive = null, end = null, limit = 1, lock = false}) {
        let encoder = new TextEncoder();
        let results = await this.scanRaw({
            start: encoder.encode(start).buffer,
            startExclusive: startExclusive !== null ? encoder.encode(startExclusive).buffer : null,
            end: end !== null ? encoder.encode(end).buffer : null,
            limit: limit,
            lock: lock,
        });
        return results.map(x => new TextDecoder().decode(x));
    }
}

const kvHandler = {
    get: function(target, prop, receiver) {
        if(prop in target) {
            return target[prop];
        } else {
            return new KvNamespace(prop);
        }
    }
}

export const kv = new Proxy({
    beginTransaction() {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: "KvBeginTransaction",
            }, [], (result) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    resolve();
                }
            })
        });
    },

    /**
     * @returns {Promise<bool>}
     */
    commit() {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: "KvCommitTransaction",
            }, [], (result) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    resolve(result.Ok.Ok);
                }
            })
        });
    },

    rollback() {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: "KvRollbackTransaction",
            }, [], (result) => {
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
}, kvHandler);

