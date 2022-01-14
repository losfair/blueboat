import { KeyInfo } from "../../jsland/dist/src/native_crypto/jwt";

Router.get("/bench/nop", async req => {
  const n = 100000;

  let start = Date.now();
  for (let i = 0; i < n; i++) {
    (<any>globalThis).__blueboat_host_invoke("nop");
  }
  let dur = Date.now() - start;
  return new Response(`${dur}ms\n`);
})

Router.get("/bench/json", async req => {
  const n = 1000;
  const payload: { numArray: number[], kvObj: Record<string, string> } = {
    numArray: new Array(100).fill(0).map(() => Math.random()),
    kvObj: Object.fromEntries(new Array(100).fill(0).map((_, i) => [`key${i}`, `value${i}`]))
  };
  const serialized = JSON.stringify(payload);
  const serializedBytes = new TextEncoder().encode(serialized);

  for (let i = 0; i < 2; i++) {

    // stringify
    let start = Date.now();
    for (let i = 0; i < n; i++) {
      JSON.stringify(payload);
    }
    let dur1a = Date.now() - start;

    start = Date.now();
    for (let i = 0; i < n; i++) {
      new TextEncoder().encode(JSON.stringify(payload));
    }
    let dur1b = Date.now() - start;

    start = Date.now()
    for (let i = 0; i < n; i++) {
      TextUtil.Json.toUint8Array(payload);
    }
    let dur2 = Date.now() - start;

    start = Date.now();
    for (let i = 0; i < n; i++) {
      JSON.parse(serialized);
    }
    let dur3 = Date.now() - start;

    start = Date.now();
    for (let i = 0; i < n; i++) {
      JSON.parse(new TextDecoder().decode(serializedBytes));
    }
    let dur4 = Date.now() - start;

    start = Date.now();
    for (let i = 0; i < n; i++) {
      TextUtil.Json.parse(serialized);
    }
    let dur5 = Date.now() - start;

    start = Date.now();
    for (let i = 0; i < n; i++) {
      TextUtil.Json.parse(serializedBytes);
    }
    let dur6 = Date.now() - start;

    if (i == 1) return new Response(JSON.stringify({
      dur1a,
      dur1b,
      dur2,
      dur3,
      dur4,
      dur5,
      dur6,
    }));
  }
  return new Response("???");
});

Router.get("/bench/textcodec", async req => {
  const naiveHex1 = (x: Uint8Array) => [...x]
    .map(b => b.toString(16).padStart(2, "0"))
    .join("");

  // https://stackoverflow.com/a/55200387
  let naiveHex2: (x: Uint8Array) => string;
  {
    const byteToHex: string[] = [];

    for (let n = 0; n <= 0xff; ++n) {
      const hexOctet = n.toString(16).padStart(2, "0");
      byteToHex.push(hexOctet);
    }

    function hex(buff: Uint8Array) {
      const hexOctets: string[] = new Array(buff.length); // new Array(buff.length) is even faster (preallocates necessary array size), then use hexOctets[i] instead of .push()

      for (let i = 0; i < buff.length; ++i)
        hexOctets[i] = byteToHex[buff[i]];

      return hexOctets.join("");
    }
    naiveHex2 = hex;
  }

  const n = 5000;
  const dataSize = 5000;
  let start = Date.now();
  {
    const buf = crypto.getRandomValues(new Uint8Array(dataSize));
    for (let i = 0; i < n; i++) {
      Codec.hexencode(buf);
    }
  }
  let dur1a = Date.now() - start;

  start = Date.now();
  {
    const buf = crypto.getRandomValues(new Uint8Array(dataSize));
    for (let i = 0; i < n; i++) {
      Codec.hexencodeToUint8Array(buf);
    }
  }
  let dur1b = Date.now() - start;

  start = Date.now();
  {
    const buf = crypto.getRandomValues(new Uint8Array(dataSize));
    for (let i = 0; i < n; i++) {
      naiveHex1(buf);
    }
  }
  let dur1_naive1 = Date.now() - start;

  start = Date.now();
  {
    const buf = crypto.getRandomValues(new Uint8Array(dataSize));
    for (let i = 0; i < n; i++) {
      naiveHex2(buf);
    }
  }
  let dur1_naive2 = Date.now() - start;

  start = Date.now();
  {
    const buf = Codec.hexencode(crypto.getRandomValues(new Uint8Array(dataSize)));
    for (let i = 0; i < n; i++) {
      Codec.hexdecode(buf);
    }
  }
  let dur2a = Date.now() - start;

  start = Date.now();
  {
    const buf = new TextEncoder().encode(Codec.hexencode(crypto.getRandomValues(new Uint8Array(dataSize))));
    for (let i = 0; i < n; i++) {
      Codec.hexdecode(buf);
    }
  }
  let dur2b = Date.now() - start;

  start = Date.now();
  {
    const buf = crypto.getRandomValues(new Uint8Array(dataSize));
    for (let i = 0; i < n; i++) {
      Codec.b64encode(buf);
    }
  }
  let dur3a = Date.now() - start;

  start = Date.now();
  {
    const buf = crypto.getRandomValues(new Uint8Array(dataSize));
    for (let i = 0; i < n; i++) {
      Codec.b64encodeToUint8Array(buf);
    }
  }
  let dur3b = Date.now() - start;

  start = Date.now();
  {
    const buf = Codec.b64encode(crypto.getRandomValues(new Uint8Array(dataSize)));
    for (let i = 0; i < n; i++) {
      Codec.b64decode(buf);
    }
  }
  let dur4a = Date.now() - start;

  start = Date.now();
  {
    const buf = new TextEncoder().encode(Codec.b64encode(crypto.getRandomValues(new Uint8Array(dataSize))));
    for (let i = 0; i < n; i++) {
      Codec.b64decode(buf);
    }
  }
  let dur4b = Date.now() - start;

  return new Response(JSON.stringify({
    dur1a,
    dur1b,
    dur1_naive1,
    dur1_naive2,
    dur2a,
    dur2b,
    dur3a,
    dur3b,
    dur4a,
    dur4b,
  }));
})

Router.get("/bench/jwt_encode", async req => {
  const n = 100000;
  const jwtKey: KeyInfo = {
    type: "base64Secret",
    data: Codec.b64encode(crypto.getRandomValues(new Uint8Array(32))),
  };
  let start = Date.now();
  for (let i = 0; i < n; i++) {
    NativeCrypto.JWT.encode({ alg: "HS256" }, {
      exp: Math.floor(Date.now() / 1000) + 60,
      hello: "world"
    }, jwtKey);
  }
  let dur = Date.now() - start;
  return new Response(`${dur}ms\n`);
})

Router.get("/bench/jwt_decode", async req => {
  const n = 100000;
  const jwtKey: KeyInfo = {
    type: "base64Secret",
    data: Codec.b64encode(crypto.getRandomValues(new Uint8Array(32))),
  };
  const enc = NativeCrypto.JWT.encode({ alg: "HS256" }, {
    exp: Math.floor(Date.now() / 1000) + 60,
    hello: "world"
  }, jwtKey);
  let start = Date.now();
  for (let i = 0; i < n; i++) {
    NativeCrypto.JWT.decode(enc, jwtKey, {
      leeway: 60,
      algorithms: ["HS256"],
    });
  }
  let dur = Date.now() - start;
  return new Response(`${dur}ms\n`);
})
