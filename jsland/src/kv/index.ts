import { wrapNativeAsync } from "../util";

export interface CompareAndSetManyRequestKey {
  key: string;
  check: TriStateCheck;
  set: TriStateSet;
}

export interface PrefixListOptions {
  reverse?: boolean;
  wantValue?: boolean;
  limit?: number;
  cursor?: string;
}

export type TriStateCheck = "absent" | "any" | { value: Uint8Array }
export type TriStateSet = "delete" | "preserve" | { value: Uint8Array } | {
  withVersionstampedKey: {
    value: Uint8Array,
  },
}

export interface CommitResult {
  versionstamp: string | null;
}

export class Namespace {
  private name: string;

  constructor(name: string) {
    this.name = name;
  }

  async getMany(paths: string[], primary: boolean = false): Promise<(Uint8Array | null)[]> {
    return <any>await wrapNativeAsync(callback => __blueboat_host_invoke("kv_get_many", {
      namespace: this.name,
      keys: paths,
      primary,
    }, callback));
  }

  async compareAndSetMany(requests: CompareAndSetManyRequestKey[]): Promise<boolean> {
    return await wrapNativeAsync(callback => __blueboat_host_invoke("kv_compare_and_set_many", {
      namespace: this.name,
      keys: requests,
    }, callback));
  }

  async compareAndSetMany1(requests: CompareAndSetManyRequestKey[]): Promise<CommitResult | null> {
    return await wrapNativeAsync(callback => __blueboat_host_invoke("kv_compare_and_set_many_1", {
      namespace: this.name,
      keys: requests,
    }, callback));
  }

  async prefixList(prefix: string, opts: PrefixListOptions = {}, primary: boolean = false): Promise<[string, Uint8Array][]> {
    opts = Object.assign(<PrefixListOptions>{
      reverse: false,
      wantValue: false,
      limit: 100,
    }, opts);
    return await wrapNativeAsync(callback => __blueboat_host_invoke("kv_prefix_list", {
      namespace: this.name,
      prefix,
      opts,
      primary,
    }, callback));
  }

  async prefixDelete(prefix: string): Promise<void> {
    await wrapNativeAsync(callback => __blueboat_host_invoke("kv_prefix_delete", {
      namespace: this.name,
      prefix,
    }, callback));
  }

  async get(path: string, primary: boolean = false): Promise<Uint8Array | null> {
    return (await this.getMany([path], primary))[0];
  }

  async set(path: string, value: Uint8Array | string): Promise<void> {
    if (typeof value === "string") {
      value = new TextEncoder().encode(value);
    }

    await this.compareAndSetMany([{
      key: path,
      check: "any",
      set: { value: value },
    }]);
  }

  async delete(path: string): Promise<void> {
    await this.compareAndSetMany([{
      key: path,
      check: "any",
      set: "delete",
    }]);
  }

  rawRun(script: string, data: string): Promise<string> {
    return wrapNativeAsync((callback) =>
      (<any>globalThis).__blueboat_host_invoke("kv_run", {
        namespace: this.name,
        script,
        data,
      }, callback)
    );
  }

  async run<T>(script: string, data: unknown): Promise<T> {
    const out = await this.rawRun(script, JSON.stringify(data));
    return JSON.parse(out);
  }
}
