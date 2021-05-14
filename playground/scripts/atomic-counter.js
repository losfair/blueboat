addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(req) {
    await kv.test.put("counter", "0", true);
    const counter = parseInt(await kv.test.get("counter"));
    const written = await kv.test.cmpUpdate([["counter", "" + counter]], [["counter", "" + (counter + 1)]]);
    if(written) {
        return new Response("Previous counter: " + counter);
    } else {
        return new Response("Write failed", { status: 500 });
    }
}
