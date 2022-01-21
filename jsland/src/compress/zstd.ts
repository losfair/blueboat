export function blockCompress(src: Uint8Array, dst?: Uint8Array | null | undefined, level?: number | null | undefined): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("compress_zstd_block_compress", src, dst, level);
}

export function blockDecompress(src: Uint8Array, dst: Uint8Array): Uint8Array {
  return <Uint8Array>__blueboat_host_invoke("compress_zstd_block_decompress", src, dst);
}
