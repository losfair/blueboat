import { CodecBase64Mode } from "../native_schema";
export * as Multipart from "./multipart";

export function hexencode(x: string | Uint8Array): string {
  return <string>__blueboat_host_invoke("codec_hexencode", x);
}

export function hexencodeToUint8Array(x: string | Uint8Array): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("codec_hexencode_to_uint8array", x);
}

export function hexdecode(x: string | Uint8Array): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("codec_hexdecode", x);
}

export function b64encode(
  x: string | Uint8Array,
  mode: CodecBase64Mode = "standard"
): string {
  return <string>__blueboat_host_invoke("codec_b64encode", x, mode);
}

export function b64encodeToUint8Array(
  x: string | Uint8Array,
  mode: CodecBase64Mode = "standard"
): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("codec_b64encode_to_uint8array", x, mode);
}

export function b64decode(
  x: string | Uint8Array,
  mode: CodecBase64Mode = "standard"
): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("codec_b64decode", x, mode);
}
