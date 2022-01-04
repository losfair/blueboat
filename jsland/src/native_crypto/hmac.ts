export function hmacSha256({ key, data }: { key: Uint8Array, data: Uint8Array }): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("crypto_hmac_sha256", {
    key,
    data,
  });
}
