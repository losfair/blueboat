addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

/**
 * 
 * @param {Request} req 
 */
async function handleRequest(req) {
    let url = new URL(req.url);

    if(url.pathname == "/api/put") {
        let text = await req.text();
        let date = new Date();
        let entry = date.toISOString();
        await kv.test.put(entry, text);
        return new Response("Entry inserted.");
    } else if(url.pathname == "/api/since") {
        let params = new URLSearchParams(url.search);
        let t = new Date(Date.now() - parseInt(params.get("t"))).toISOString();
        let n = parseInt(params.get("n"));
        let now = new Date().toISOString();
        let batchSize = 5;
        let result = [];
        let last = "";
        for(let i = 0; i < n; i += batchSize) {
            let limit = n - i > batchSize ? batchSize : n - i;
            let keys = await kv.test.scan({
                start: last ? "" : t,
                startExclusive: last ? last : null,
                end: now,
                limit: limit,
            });
            if(keys.length) {
                for(let key of keys) {
                    let value = await kv.test.get(key);
                    result.push({
                        date: key,
                        text: value,
                    });
                }
                last = keys[keys.length - 1];
            } else {
                break;
            }
        }
        return new Response(JSON.stringify(result));
    }
}
