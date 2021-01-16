addEventListener("fetch", async (event) => {
    event.respondWith(handleRequest(event.request));
});

/**
 * 
 * @param {Request} req 
 */
async function handleRequest(req) {
    let body = await req.arrayBuffer();
    let digest = await crypto.subtle.digest("SHA-256", body);
    return new Response(bufferToHex(digest));
}

// https://stackoverflow.com/questions/40031688/javascript-arraybuffer-to-hex
function bufferToHex (buffer) {
    return [...new Uint8Array (buffer)]
        .map (b => b.toString (16).padStart (2, "0"))
        .join ("");
}
