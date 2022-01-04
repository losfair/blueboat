export * as Ed25519 from "./ed25519";
export * as X25519 from "./x25519";
export * as JWT from "./jwt";
export * as AEAD from "./aead";
export * as HMAC from "./hmac";

export type DigestAlgorithm = "sha1" | "sha256" | "sha384" | "sha512" | "blake3";

export function digest(
  algorithm: DigestAlgorithm,
  data: Uint8Array
): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("crypto_digest", algorithm, data);
}

export function constantTimeEq(a: Uint8Array, b: Uint8Array): boolean {
  return <boolean>__blueboat_host_invoke("crypto_constant_time_eq", a, b);
}
