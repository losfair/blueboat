export * as Ed25519 from "./ed25519";
export * as X25519 from "./x25519";
export * as JWT from "./jwt";

export type DigestAlgorithm = "sha1" | "sha256" | "sha384" | "sha512";

export function digest(
  algorithm: DigestAlgorithm,
  data: Uint8Array
): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("crypto_digest", algorithm, data);
}
