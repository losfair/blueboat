export function render(
  src: string,
  context: Record<string, unknown>,
  disableAutoescape: boolean = false
): string {
  return <string>(
    __blueboat_host_invoke("tera_render", src, context, disableAutoescape)
  );
}
