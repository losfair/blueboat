addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(req) {
    await new Promise(resolve => setTimeout(resolve, 25000));
    return new Response("Done");
}
