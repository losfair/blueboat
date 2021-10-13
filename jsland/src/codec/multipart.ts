export interface MultipartField {
  name: string | null | undefined;
  file_name: string | null | undefined;
  content_type: string | null | undefined;
  body: Uint8Array;
}

export function decode(data: Uint8Array, boundary: string): MultipartField[] {
  return <MultipartField[]>(
    __blueboat_host_invoke("codec_multipart_decode", data, boundary)
  );
}
