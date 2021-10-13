import { KeyInfo } from "../../jsland/dist/src/native_crypto/jwt";

Router.get("/crypto", req => {
  try {
    const edSec = new NativeCrypto.Ed25519.SecretKey();
    const edPub = new NativeCrypto.Ed25519.PublicKey(edSec);
    const xSec = edSec.convertToX25519();
    const xPubPath1 = new NativeCrypto.X25519.PublicKey(xSec);
    const xPubPath2 = edPub.convertToX25519();
    const edPubRoundtrip = xPubPath2.convertToEd25519();
    const edKp = new NativeCrypto.Ed25519.Keypair(edSec, edPub);
  
    const signPayload = crypto.getRandomValues(new Uint8Array(10));
    const sig = edKp.sign(signPayload);
    const verifyResult = edPub.verify(sig, signPayload);
    const verifyResultStrict = edPub.verify(sig, signPayload, true);

    const xSec2 = new NativeCrypto.X25519.SecretKey();
    const xPub2 = new NativeCrypto.X25519.PublicKey(xSec2);
    const dhWay1 = xSec2.diffieHellman(xPubPath1);
    const dhWay2 = xSec.diffieHellman(xPub2);
  
    const res = `
edSec: ${Codec.b64encode(edSec.exportSecret())}
edPub: ${Codec.b64encode(edPub.exportPublic())}
xSec: ${Codec.b64encode(xSec.exportSecret())}
xPubPath1: ${Codec.b64encode(xPubPath1.exportPublic())}
xPubPath2: ${Codec.b64encode(xPubPath2.exportPublic())}
edPubRoundtrip: ${Codec.b64encode(edPubRoundtrip.exportPublic())}
signPayload: ${Codec.b64encode(signPayload)}
sig: ${Codec.b64encode(sig)}
verifyResult: ${verifyResult}
verifyResultStrict: ${verifyResultStrict}
xSec2: ${Codec.b64encode(xSec2.exportSecret())}
xPub2: ${Codec.b64encode(xPub2.exportPublic())}
dhWay1: ${Codec.b64encode(dhWay1)}
dhWay2: ${Codec.b64encode(dhWay2)}
  `;
    return new Response(res.trim() + "\n", {
      headers: {
        "Content-Type": "text/plain"
      }
    });
  } catch(e) {
    return new Response("" + e.stack, {
      status: 500,
      headers: {
        "Content-Type": "text/plain"
      }
    });
  }
});

const jwtKey: KeyInfo = {
  type: "base64Secret",
  data: Codec.b64encode(new TextEncoder().encode("secret")),
};

Router.get("/jwt/encode", req => {
  const res = NativeCrypto.JWT.encode({alg: "HS256"}, {
    exp: Math.floor(Date.now() / 1000) + 60,
    hello: "world"
  }, jwtKey);
  return new Response(res);
})

Router.post("/jwt/decode", async req => {
  const token = await req.text();
  try {
    const res = NativeCrypto.JWT.decode(token, jwtKey, {
      leeway: 60,
      algorithms: ["HS256"],
    });
    return new Response(JSON.stringify(res, null, 2));
  } catch(e) {
    return new Response("" + e, {
      status: 400,
    });
  }
})