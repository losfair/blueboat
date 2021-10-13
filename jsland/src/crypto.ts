export function getRandomValues<
  T extends
    | Int8Array
    | Uint8Array
    | Int16Array
    | Uint16Array
    | Int32Array
    | Uint32Array
>(out: T): T {
  return <T>__blueboat_host_invoke("crypto_getrandom", out);
}
