addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(req) {
    await kv.beginTransaction();
    let counter = await kv.test.get("counter", true);
    if(counter === null) {
        counter = 0;
    } else {
        counter = parseInt(counter);
    }

    counter += 1;

    await kv.test.put("counter", "" + counter);
    let written = await kv.commit();
    if(written) {
        return new Response("New counter: " + counter);
    } else {
        return new Response("Write failed");
    }
}
