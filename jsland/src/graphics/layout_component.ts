import { CanvasRenderingContext2DImpl } from "./canvas";
import { DrawingMetrics } from "./layout_draw";

export interface Component {
  draw(canvas: CanvasRenderingContext2DImpl, metrics: DrawingMetrics): void;
}

export class Rectangle implements Component {
  fillStyle: string;

  constructor(fillStyle: string) {
    this.fillStyle = fillStyle;
  }

  draw(canvas: CanvasRenderingContext2DImpl, metrics: DrawingMetrics): void {
    canvas.fillStyle = this.fillStyle;
    canvas.fillRect(
      metrics.left,
      metrics.top,
      metrics.right - metrics.left,
      metrics.bottom - metrics.top
    );
  }
}
