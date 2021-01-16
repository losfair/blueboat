const DIGEST_MAP = {
    "SHA-1": "Sha1",
    "SHA-256": "Sha256",
    "SHA-384": "Sha384",
    "SHA-512": "Sha512",
};

export function digest(algorithm, data) {
    return new Promise((resolve, reject) => {
        queueMicrotask(() => {
            let targetAlg = DIGEST_MAP[algorithm];
            if(!targetAlg) {
                return reject(new Error("digest: bad algorithm"));
            }
            try {
                resolve(_callServiceWrapper({
                    Sync: {
                        Crypto: {
                            Digest: targetAlg,
                        }
                    }
                }, [data]));
            } catch(e) {
                reject(e);
            }
        });
    });
}