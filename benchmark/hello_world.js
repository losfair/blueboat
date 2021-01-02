console.log("hello_world loaded");

addEventListener("fetch", (event) => {
    event.respondWith(handleRequest(event.request));
});

function handleRequest(req) {
    return new Response("Hello, world! Request:" + JSON.stringify(req));
}