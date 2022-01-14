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
