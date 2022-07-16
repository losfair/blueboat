import { BlueboatBootstrapData } from "./native_schema";

export { mysql } from "./mysql";
export { apns } from "./apns";
export { pubsub } from "./pubsub";

import { init as mysqlInit } from "./mysql";
import { init as apnsInit } from "./apns";
import { init as pubsubInit } from "./pubsub/index";

export { serveStaticFiles } from "./serve_static";

export const env: Record<string, string> = {};

export function mustGetEnv(key: string): string {
  const v = env[key];
  if (typeof v === "string") {
    return v;
  } else {
    throw new Error(`string value not found in env for key '${key}'`);
  }
}

export function init(bs: BlueboatBootstrapData) {
  Object.assign(env, bs.env);
  mysqlInit(bs);
  apnsInit(bs);
  pubsubInit(bs);
}
