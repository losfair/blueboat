/// <reference path="../../jsland/dist/src/index.d.ts" />

import { sayHelloWorld } from "./subdir/hello.js";
import "./graphics.ts";
import "./crypto.ts";
import "./validation.ts";
import { Solver } from "../../jsland/dist/src/graphics/layout_solver.js";
import { DrawingMetrics } from "../../jsland/dist/src/graphics/layout_draw.js";

class BgEntry extends Background.BackgroundEntryBase {
  constructor() {
    super();
  }

  testLog(text: string) {
    console.log(text);
  }
}
const bgEntry = new BgEntry();

Router.get("/", req => new Response(sayHelloWorld()));
Router.get("/exception", req => {
  throw new Error("test exception");
});
Router.get("/embedded", req => new Response(Package["embedded.txt"]));
Router.get("/embedded2", req => new Response(Package["embedded.txt"]));
Router.get("/sleep", req => new Promise(resolve => setTimeout(() => resolve(new Response("done")), 1000)));
Router.get("/headers", req => new Response(JSON.stringify([...req.headers.entries()], null, 2)));
Router.get("/middleware", req => new Response(req.headers.get("x-test-header") || ""));
Router.get("/template", req => new Response(Template.render(
  "the current time is {{ now }} and some escaped html is {{ text }}\n",
  {
    now: Date.now(),
    text: "<script>alert('test')</script>",
  }
)));
Router.get("/multipart", req => {
  const data = "--X-BOUNDARY\r\nContent-Disposition: form-data; name=\"my_text_field\"\r\n\r\nabcd\r\n--X-BOUNDARY--\r\n";
  const boundary = "X-BOUNDARY";
  const res = Codec.Multipart.decode(new TextEncoder().encode(data), boundary);
  return new Response(JSON.stringify(res, null, 2));
});
Router.get("/mime_by_ext", req => {
  const url = new URL(req.url);
  const res = Dataset.Mime.guessByExt(url.searchParams.get("ext") || "") || "";
  return new Response(res + "\n");
});
Router.get("/background/atMostOnce", req => {
  console.log("scheduling background task (atMostOnce)");
  Background.atMostOnce(bgEntry, "testLog", "hello from background (atMostOnce)");
  return new Response("Scheduled.\n");
});
Router.get("/background/atLeastOnce", async req => {
  console.log("scheduling background task (atLeastOnce)");
  await Background.atLeastOnce(bgEntry, "testLog", "hello from background (atLeastOnce)");
  return new Response("Scheduled.\n");
});
Router.get("/background/delayed", async req => {
  console.log("scheduling background task (delayed)");
  let { id } = await Background.delayed(bgEntry, "testLog", "hello from background (delayed)", {
    tsSecs: Date.now() / 1000 + 5,
  });
  return new Response(`Scheduled. ID: ${id}\n`);
});
Router.get("/markdown", req => {
  return new Response(TextUtil.Markdown.renderToHtml("# Hello world\n\nTest paragraph. <script></script>", {}), {
    headers: {
      "Content-Type": "text/html",
    },
  });
})
Router.get("/constraints", req => {
  const solver: Solver<string> = new Graphics.Layout.Solver();
  const boxA = solver.box("a");
  const boxB = solver.box("b");
  const combined = new Graphics.Layout.Box.Combined(solver, [boxA, boxB]);
  combined.horizontallyAlign("middle");
  const model = solver.solve();
  if (typeof model == "string") {
    return new Response(model);
  }

  const output: { component: string, metrics: DrawingMetrics }[] = [];
  Graphics.Layout.draw(model, (component, metrics) => {
    output.push({
      component,
      metrics,
    });
  })
  const numUnsats = model.unsat.length;
  return new Response(JSON.stringify({
    output,
    numUnsats,
  }, null, 2))
});
Router.get("/yaml/parse", req => {
  const res = TextUtil.Yaml.parse(`
map:
  a: b
  c:
  - 1
  - 2
  - 3
  `);
  return new Response(JSON.stringify(res, null, 2));
});
Router.get("/yaml/stringify", req => {
  const res = TextUtil.Yaml.stringify({
    hello: "world",
  });
  return new Response(res);
})

Router.get("/never", req => {
  while (true);
});

Router.use("/", async (req, next) => {
  const res = await next(req);
  res.headers.set("x-root-middleware", "1");
  return res;
});

Router.use("/middleware", async (req, next) => {
  req.headers.set("x-test-header", "test-header");
  const res = await next(req);
  res.headers.set("x-leaf-middleware", "1");
  return res;
});

console.log("build time console log");

Router.get("/kv/get", async req => {
  const ns = new KV.Namespace("ns1");
  const v = await ns.get("key1");
  const s = v === null ? null : new TextDecoder().decode(v);
  return new Response(JSON.stringify({ value: s, ok: true }));
});

Router.get("/kv/set", async req => {
  const ns = new KV.Namespace("ns1");
  await ns.set("key1", "" + Date.now());
  return new Response(JSON.stringify({ ok: true }));
});

Router.get("/kv/vs", async req => {
  const ns = new KV.Namespace("ns1");
  const res = await ns.compareAndSetMany1([
    {
      key: "seqlog",
      check: "any",
      set: {
        withVersionstampedKey: {
          value: new TextEncoder().encode("hello"),
        },
      }
    }
  ]);
  return new Response(JSON.stringify(res));
});


Router.get("/kv/list", async req => {
  const ns = new KV.Namespace("ns1");
  const v = await ns.prefixList("");
  return new Response(JSON.stringify({ value: v, ok: true }));
});

Router.get("/kv/pd", async req => {
  const ns = new KV.Namespace("ns1");
  await ns.prefixDelete("");
  return new Response(JSON.stringify({ ok: true }));
});

Router.get("/random_uuid", () => new Response((<any>crypto).randomUUID()));

Router.get("/web/", App.serveStaticFiles("/web/", "static"));
Router.setDebugPath("/debug");

Router.get("/measure_text", req => {
  const u = new URL(req.url);
  const text = u.searchParams.get("text") || "";
  const width = parseFloat(u.searchParams.get("width") || "0");
  const m = Graphics.Text.measureSimple(text, {
    maxWidth: width,
    font: "20px roboto, sans-serif",
  });
  return new Response(JSON.stringify(m, null, 2), {
    headers: {
      "Content-Type": "application/json",
    },
  });
});
