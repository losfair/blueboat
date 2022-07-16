import { BlueboatBootstrapData, BlueboatRequest, BlueboatResponse } from "../native_schema";
import { generateStdRequest, wrapNativeAsync } from "../util";

export type ClientAuthorizationHandler = (req: Request, ctx: ClientAuthorizationContext) => Promise<boolean>;

export interface Pubsub {
  publish(request: PublishRequest, data: Uint8Array | string): Promise<PublishResult>;
  authorizeClient(handler: ClientAuthorizationHandler): void;
}

export type PublishRequest = {
  topic: string;
} | string;

export interface PublishResult {
  id: string;
}

export interface ClientAuthorizationContext {
  topic: string;
}

class PubsubImpl implements Pubsub {
  authorizer: ClientAuthorizationHandler | null = null;

  constructor(private key: string) { }

  async publish(request: PublishRequest, data: string | Uint8Array): Promise<PublishResult> {
    let topic = typeof request === "string" ? request : request.topic;
    return await wrapNativeAsync(callback => __blueboat_host_invoke("pubsub_publish", {
      namespace: this.key,
      topic,
    }, data, callback));
  }

  authorizeClient(handler: ClientAuthorizationHandler): void {
    this.authorizer = handler;
  }
}

export const pubsub: Record<string, Pubsub> = {};

export function init(bs: BlueboatBootstrapData) {
  for (const x of bs.pubsub) {
    pubsub[x] = new PubsubImpl(x);
  }
}

export async function appSseAuthEntry(req: BlueboatRequest): Promise<void> {
  try {
    const stdRes = await handleSseAuth(req);
    const res: BlueboatResponse = {
      status: stdRes.status,
      headers: {},
    };
    __blueboat_host_invoke("complete", res, new Uint8Array());
  } catch (e) {
    console.log("sse auth error: " + e);
    const res: BlueboatResponse = {
      status: 500,
      headers: {},
    };
    __blueboat_host_invoke("complete", res, new Uint8Array());
  }
}

async function handleSseAuth(req: BlueboatRequest): Promise<Response> {
  let stdReq = generateStdRequest(req, null);
  let url = new URL(stdReq.url);

  let ns = url.searchParams.get("ns");
  if (!ns) return new Response("missing ns", { status: 400 });

  let topic = url.searchParams.get("topic");
  if (!topic) return new Response("missing topic", { status: 400 });

  let impl = pubsub[ns] as PubsubImpl | undefined;
  if (!impl || !impl.authorizer) {
    return new Response("namespace not found", { status: 404 });
  }
  let ok = await impl.authorizer(stdReq, {
    topic,
  })
  if (ok) {
    return new Response("ok", { status: 200 });
  } else {
    return new Response("forbidden", { status: 403 });
  }
}
