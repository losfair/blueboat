import { BlueboatResponse } from "../native_schema";
import { wrapNativeAsync } from "../util";

let registration: BackgroundEntryBase | null = null;

export interface AtLeastOnceOpts {
  sameVersion?: boolean;
}

export interface DelayedTaskOpts {
  tsSecs: number;
  sameVersion?: boolean;
}

export interface DelayedTaskInfo {
  id: string;
}

export class BackgroundEntryBase {
  constructor() {
    if (registration)
      throw new Error("multiple BackgroundEntryBase registrations");
    registration = this;
  }
}

interface BackgroundInvocation {
  entry: string;
  arg: unknown;
}

export function atMostOnce<
  A,
  T extends BackgroundEntryBase & { [P in K]: (arg: A) => unknown },
  K extends keyof T & string
>(base: T, key: K, arg: A) {
  const inv: BackgroundInvocation = {
    entry: key,
    arg,
  };
  __blueboat_host_invoke("schedule_at_most_once", inv);
}

export function atLeastOnce<
  A,
  T extends BackgroundEntryBase & { [P in K]: (arg: A) => unknown },
  K extends keyof T & string
>(base: T, key: K, arg: A, opts: AtLeastOnceOpts = {}): Promise<void> {
  const inv: BackgroundInvocation = {
    entry: key,
    arg,
  };
  return wrapNativeAsync((callback) =>
    __blueboat_host_invoke(
      "schedule_at_least_once",
      inv,
      opts,
      callback
    )
  );
}

export function delayed<
  A,
  T extends BackgroundEntryBase & { [P in K]: (arg: A) => unknown },
  K extends keyof T & string
>(base: T, key: K, arg: A, opts: DelayedTaskOpts): Promise<DelayedTaskInfo> {
  const inv: BackgroundInvocation = {
    entry: key,
    arg,
  };
  return wrapNativeAsync((callback) =>
    __blueboat_host_invoke(
      "schedule_delayed",
      inv,
      opts,
      callback
    )
  );
}

export async function appBackgroundEntry(message: BackgroundInvocation) {
  try {
    let f: (arg: unknown) => unknown = (<any>registration)[message.entry];
    await f(message.arg);
  } catch (e) {
    console.log("background entry error: " + e);
  }
  let res: BlueboatResponse = {
    status: 200,
    headers: {},
  };
  __blueboat_host_invoke("complete", res, new Uint8Array());
}
