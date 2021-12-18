import { GraphicsTextMeasureOutput, GraphicsTextMeasureSettings } from "../native_schema";

export interface MeasureOpts {
  font: string;
  maxWidth: number;
}

export function measureSimple(text: string, opts: MeasureOpts): { height: number, lines: number } {
  const settings: GraphicsTextMeasureSettings = {
    text,
    font: opts.font,
    max_width: opts.maxWidth,
  };
  const out = <GraphicsTextMeasureOutput>__blueboat_host_invoke("graphics_text_measure", settings);
  return out;
}