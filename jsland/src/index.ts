import * as appMod from "./app";
import * as routerMod from "./router";
import {
  BlueboatBootstrapData,
  BlueboatRequest,
  BlueboatResponse,
} from "./native_schema";
import * as codecMod from "./codec/index";
import * as graphicsMod from "./graphics";
import * as nativeCryptoMod from "./native_crypto/index";
import {
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
} from "./timeout";
import * as templateMod from "./template";
import * as validationMod from "./validation";
import * as extSvcMod from "./external_service/index";
import * as datasetMod from "./dataset/index";
import * as backgroundMod from "./background/index";
import { appBackgroundEntry } from "./background/impl";
import * as textMod from "./text/index";
import * as kvMod from "./kv";
import { HostObject as HostObject_ } from "./host_object"
import * as compressMod from "./compress/index";

const lateRoot = {
  App: appMod,
  Graphics: graphicsMod,
  NativeCrypto: nativeCryptoMod,
  Router: routerMod.router,
  Template: templateMod,
  Validation: validationMod,
  ExternalService: extSvcMod,
  Codec: codecMod,
  Dataset: datasetMod,
  Background: backgroundMod,
  TextUtil: textMod,
  KV: kvMod,
  Compress: compressMod,
  HostObject: HostObject_,
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
  __blueboat_app_entry: appEntry,
  __blueboat_app_background_entry: appBackgroundEntry,
  __blueboat_app_bootstrap: appBootstrap,
};

const methodMap: Record<
  string,
  (x: routerMod.Route) => routerMod.HttpHandler | undefined
> = {
  GET: (x) => x.get,
  HEAD: (x) => x.head || x.get,
  POST: (x) => x.post,
  PUT: (x) => x.put,
  PATCH: (x) => x.patch,
  DELETE: (x) => x.delete,
  OPTIONS: (x) => x.options,
};

async function appEntry(req: BlueboatRequest, body: ArrayBuffer) {
  try {
    await realAppEntry(req, body);
  } catch (e) {
    console.log(`unhandled error from realAppEntry (${e}): ${e.stack}`);
  }
}

function appBootstrap(data: BlueboatBootstrapData) {
  appMod.init(data);
}

async function realAppEntry(req: BlueboatRequest, body: ArrayBuffer) {
  req.uri =
    "https://" + (req.headers.host ? req.headers.host[0] : "nohost") + req.uri;
  let stdReq = new Request(req.uri, {
    method: req.method,
    headers: Object.keys(req.headers).map((k) => [
      k,
      req.headers[k].join(", "),
    ]),
    body: body.byteLength == 0 ? null : body,
  });
  const url = new URL(req.uri);
  const routeInfo = routerMod.coreRouter.lookupChild(url.pathname);
  let res: BlueboatResponse;
  let resBody: Uint8Array;
  if (routeInfo && methodMap[req.method](routeInfo[0])) {
    const [route, mw] = routeInfo;
    try {
      let handleFunc = methodMap[req.method](route)!;
      for (const x of mw) {
        for (const y of x) {
          const currentMw = y;
          const currentHandler = handleFunc;
          handleFunc = (req) => {
            return currentMw(req, currentHandler);
          };
        }
      }
      const stdRes = await handleFunc(stdReq);
      res = {
        headers: {},
        status: stdRes.status,
      };
      stdRes.headers.forEach((v, k) => {
        res.headers[k] = [v];
      });
      const body = await stdRes.arrayBuffer();
      resBody = new Uint8Array(body);
    } catch (e) {
      console.log(`error handling path ${url.pathname} (${e}): ${e.stack}`);
      res = {
        status: 500,
        headers: {
          "Content-Type": ["text/html"],
        },
      };

      const reqId = stdReq.headers.get("x-blueboat-request-id") || "";
      const reqTime = Date.now();

      let traceUrl: URL | null = null;
      const analyticsUrl: string | undefined = (<any>globalThis).__blueboat_env_analytics_url;
      if (typeof analyticsUrl === "string") {
        traceUrl = new URL(analyticsUrl);
        traceUrl.searchParams.set("id", reqId);
        traceUrl.searchParams.set("t", "" + reqTime);
      }
      resBody = new TextEncoder().encode(
        Template.render(
          `
<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <link rel="stylesheet" href="https://cdn.staticfile.org/tailwindcss/2.2.17/tailwind.min.css" integrity="sha512-yXagpXH0ulYCN8G/Wl7GK+XIpdnkh5fGHM5rOzG8Kb9Is5Ua8nZWRx5/RaKypcbSHc56mQe0GBG0HQIGTmd8bw==" crossorigin="anonymous" referrerpolicy="no-referrer" />
    <title>Blueboat: Application exception</title>
  </head>
  <body>
    <div class="bg-gray-50">
      <div class="flex flex-col h-screen space-y-3 py-20 px-8 md:px-16 lg:px-28 text-gray-600">
        <p>The application thrown an exception while handling your request.</p>
        {% if traceUrl %}
        <p>Please check the <a href="{{traceUrl}}" class="text-blue-600">request trace</a>.</p>
        {% endif %}
        <div class="flex flex-col pt-10 space-y-3 text-gray-400 text-sm">
          <div>Request ID: {{reqTime}}/{{reqId}}</div>
          <div><a href="https://github.com/losfair/blueboat" class="hover:text-gray-500">Blueboat v{{version}}</a></div>
        </div>
      </div>
    </div>
  </body>
</html>`.trim(),
          {
            reqId,
            traceUrl: traceUrl?.toString(),
            reqTime,
            version: "" + (<any>globalThis).__blueboat_version,
          }
        )
      );
    }
  } else {
    res = {
      status: 404,
      headers: {},
    };
    resBody = new TextEncoder().encode("not found");
  }
  __blueboat_host_invoke("complete", res, resBody);
}

Object.assign(globalThis, lateRoot);

declare global {
  const Router: routerMod.Router;
  const App: typeof appMod;
  const Codec: typeof codecMod;
  const Graphics: typeof graphicsMod;
  const NativeCrypto: typeof nativeCryptoMod;
  const Package: Record<string, Uint8Array>;
  const Template: typeof templateMod;
  const Validation: typeof validationMod;
  const ExternalService: typeof extSvcMod;
  const Dataset: typeof datasetMod;
  const Background: typeof backgroundMod;
  const TextUtil: typeof textMod;
  const KV: typeof kvMod;
  const HostObject: typeof HostObject_;
  const Compress: typeof compressMod;
}
