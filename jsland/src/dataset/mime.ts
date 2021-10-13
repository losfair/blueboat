export function guessByExt(ext: string): string | undefined {
  return <string | undefined>(
    __blueboat_host_invoke("dataset_mime_guess_by_ext", ext)
  );
}
