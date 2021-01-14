addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(req) {
    return new Response("Hello, world!");
}
