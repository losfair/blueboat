import { TextMarkdownRenderOpts } from "../native_schema";

export function renderToHtml(
  text: string,
  opts: TextMarkdownRenderOpts
): string {
  return <string>__blueboat_host_invoke("text_markdown_render", text, opts);
}
