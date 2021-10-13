import { CodecBase64Mode } from "../native_schema";
export * as Multipart from "./multipart";

export function hexencode(x: Uint8Array): string {
  return <string>__blueboat_host_invoke("codec_hexencode", x);
}

export function hexdecode(x: string): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("codec_hexdecode", x);
}

export function b64encode(
  x: Uint8Array,
  mode: CodecBase64Mode = "standard"
): string {
  return <string>__blueboat_host_invoke("codec_b64encode", x, mode);
}

export function b64decode(
  x: string,
  mode: CodecBase64Mode = "standard"
): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("codec_b64decode", x, mode);
}
