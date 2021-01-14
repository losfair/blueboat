
addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

async function handleRequest(req) {
    let arr = [];
    for(let i = 0;; i++) arr.push(new ArrayBuffer(1000000));
}
