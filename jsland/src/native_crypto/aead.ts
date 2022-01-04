export interface AesGcmSivParams {
  key: Uint8Array;
  nonce: Uint8Array;
  data: Uint8Array;
  associatedData?: Uint8Array | null | undefined,
}

export function aes128GcmSivEncrypt(params: AesGcmSivParams): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("crypto_aead_aes128_gcm_siv_encrypt", params);
}

export function aes128GcmSivDecrypt(params: AesGcmSivParams): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("crypto_aead_aes128_gcm_siv_decrypt", params);
}
