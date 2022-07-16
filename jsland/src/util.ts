import { BlueboatRequest } from "./native_schema";

export type NativeAsyncCallback<T> = (
  err: Error | undefined,
  result: T | undefined
) => void;

export async function wrapNativeAsync<T>(
  f: (callback: NativeAsyncCallback<T>) => void
): Promise<T> {
  let nativeRejection = false;
  try {
    return await new Promise((resolve, reject) => {
      f((err, res) => {
        if (err) {
          nativeRejection = true;
          reject(err);
        } else {
          resolve(res!);
        }
      });
    });
  } catch (e) {
    if (e instanceof Error && nativeRejection) {
      try {
        throw new Error();
      } catch (that) {
        e.stack = that.stack;
      }
    }
    throw e;
  }
}

export function generateStdRequest(req: BlueboatRequest, body: ArrayBuffer | null): Request {
  let url = "https://" + (req.headers.host ? req.headers.host[0] : "nohost") + req.uri;
  let stdReq = new Request(url, {
    method: req.method,
    headers: Object.keys(req.headers).map((k) => [
      k,
      req.headers[k].join(", "),
    ]),
    body: body ? (body.byteLength == 0 ? null : body) : null,
  });
  return stdReq;
}
