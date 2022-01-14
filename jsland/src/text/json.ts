export function parse(x: string | Uint8Array): unknown {
  return __blueboat_host_invoke("text_json_parse", x);
}

type ToUint8ArrayOutput<T> = T extends undefined ? undefined : Uint8Array;

export function toUint8Array<T>(x: T): ToUint8ArrayOutput<T> {
  return <ToUint8ArrayOutput<T>>__blueboat_host_invoke("text_json_to_uint8array", x);
}
