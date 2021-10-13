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
    let handleFunc = methodMap[req.method](route)!;
    try {
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
        headers: {},
      };
      resBody = new TextEncoder().encode("internal error");
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
}
