import { wrapNativeAsync } from "../util";

export interface CompareAndSetManyRequestKe {
  key: string;
  check: TriStateCheck;
  set: TriStateSet;
}

export type TriStateCheck = "absent" | "any" | { value: Uint8Array }
export type TriStateSet = "delete" | "preserve" | { value: Uint8Array }

export class Namespace {
  private name: string;

  constructor(name: string) {
    this.name = name;
  }

  async get(path: string, primary: boolean = false): Promise<Uint8Array | null> {
    return (<any>await wrapNativeAsync(callback => __blueboat_host_invoke("kv_get_many", {
      namespace: this.name,
      keys: [path],
      primary,
    }, callback)))[0];
  }

  async getMany(paths: string[], primary: boolean = false): Promise<(Uint8Array | null)[]> {
    return <any>await wrapNativeAsync(callback => __blueboat_host_invoke("kv_get_many", {
      namespace: this.name,
      keys: paths,
      primary,
    }, callback));
  }

  async set(path: string, value: Uint8Array | string): Promise<void> {
    if (typeof value === "string") {
      value = new TextEncoder().encode(value);
    }

    await wrapNativeAsync(callback => __blueboat_host_invoke("kv_compare_and_set_many", {
      namespace: this.name,
      keys: <CompareAndSetManyRequestKe[]>[
        {
          key: path,
          check: "any",
          set: { value: value },
        }
      ],
    }, callback));
  }
}