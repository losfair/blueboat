import {
  ApnsRequest,
  ApnsResponse,
  BlueboatBootstrapData,
} from "./native_schema";

export interface Apns {
  send(data: ApnsRequest): Promise<ApnsResponse>;
}

class ApnsImpl implements Apns {
  private key: string;

  constructor(key: string) {
    this.key = key;
  }

  async send(data: ApnsRequest): Promise<ApnsResponse> {
    let nativeRejection = false;
    try {
      return await new Promise((resolve, reject) => {
        __blueboat_host_invoke(
          "apns_send",
          this.key,
          data,
          (err: Error | undefined, res: ApnsResponse | undefined) => {
            if (err) {
              nativeRejection = true;
              reject(err);
            } else {
              let out = res!;
              resolve(out);
            }
          }
        );
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
}

export const apns: Record<string, Apns> = {};

export function init(bs: BlueboatBootstrapData) {
  for (const x of bs.apns) {
    apns[x] = new ApnsImpl(x);
  }
}
