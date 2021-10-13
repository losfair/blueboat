import { SolveResult } from "./layout_solver";

export interface DrawingMetrics {
  left: number;
  top: number;
  bottom: number;
  right: number;
  z: number;
}

export function draw<C>(
  solved: SolveResult<C>,
  drawFunc: (component: C, metrics: DrawingMetrics) => void
) {
  const boxes = Array.from(solved.boxes);
  boxes.sort(([_kl, l], [_kr, r]) => l.z - r.z);

  for (const [k, v] of boxes) {
    drawFunc(k.component, v);
  }
}
