addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(req) {
    let counter = await kv.testNs.get("counter");
    if(counter === null) {
        counter = 0;
    } else {
        counter = parseInt(counter);
    }

    counter += 1;

    await kv.testNs.put("counter", "" + counter);
    return new Response("New counter: " + counter);
}
