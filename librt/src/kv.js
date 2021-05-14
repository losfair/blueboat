

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
     * @param {boolean} ifNotExists
     * @returns {Promise<void>}
     */
    putRaw(key, value, ifNotExists = false) {
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: {
                    KvPut: {
                        namespace: this.name,
                        if_not_exists: ifNotExists,
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
     * @param {boolean} ifNotExists
     * @returns {Promise<void>}
     */
    async put(key, value, ifNotExists = false) {
        let keyRaw = new TextEncoder().encode(key);
        let valueRaw = new TextEncoder().encode(value);
        await this.putRaw(keyRaw.buffer, valueRaw.buffer, ifNotExists);
    }

    /**
     * 
     * @param {[ArrayBuffer | ArrayBufferView, ArrayBuffer | ArrayBufferView][]} assertions 
     * @param {[ArrayBuffer | ArrayBufferView, ArrayBuffer | ArrayBufferView][]} writes
     * @returns {Promise<boolean>}
     */
    cmpUpdateRaw(assertions, writes) {
        const bufferList = [];
        for(const [k, v] of assertions) {
            bufferList.push(k);
            bufferList.push(v);
        }

        for(const [k, v] of writes) {
            bufferList.push(k);
            bufferList.push(v);
        }
        return new Promise((resolve, reject) => {
            _callServiceWrapper({
                Async: {
                    KvCmpUpdate: {
                        namespace: this.name,
                        num_assertions: assertions.length,
                        num_writes: writes.length,
                    }
                }
            }, bufferList, (result) => {
                if(result.Err) {
                    reject(new Error(result.Err));
                } else if(result.Ok.Err) {
                    reject(new Error(result.Ok.Err));
                } else {
                    resolve(result.Ok.Ok);
                }
            })
        });
    }

        /**
     * 
     * @param {[string, string][]} assertions 
     * @param {[string, string][]} writes
     * @returns {Promise<boolean>}
     */
    async cmpUpdate(assertions, writes) {
        const encoder = new TextEncoder();
        return await this.cmpUpdateRaw(
            assertions.map(x => x.map(x => encoder.encode(x))),
            writes.map(x => x.map(x => encoder.encode(x))),
        );
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

export const kv = new Proxy({}, kvHandler);
