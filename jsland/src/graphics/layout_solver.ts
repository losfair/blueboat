import * as Native from "./layout_native";
import * as Util from "./layout_util";
import * as Box from "./layout_box";

export interface SolveResult<C> {
  unsat: Native.Constraint<Box.Bare<C, Solver<C>>>[];
  boxes: Map<
    Box.Bare<C, Solver<C>>,
    Native.BoxSolution<Box.Bare<C, Solver<C>>>
  >;
}

export class Solver<C> implements Box.Context<C> {
  request: Native.Request<Box.Bare<C, Solver<C>>> = {
    boxes: [],
    metrics: [],
    constraints: [],
  };

  constructor() {}

  box(component: C): Box.Box<C, this> {
    const box = new Box.Bare<C, this>(
      this,
      this.request.boxes.length,
      component
    );
    this.pushConstraint({
      prop: Util.realCmp("ge", box.right(), box.left()),
      weight: 0,
    });
    this.pushConstraint({
      prop: Util.realCmp("ge", box.bottom(), box.top()),
      weight: 0,
    });
    this.request.boxes.push(box);
    return box;
  }

  traceConstraintToBox(
    constraint: Native.Constraint<Box.Box<C, Solver<C>>>
  ): [Box.Box<C, Solver<C>>, Native.LayoutBoxProperty][] {
    const out: [Box.Box<C, Solver<C>>, Native.LayoutBoxProperty][] = [];
    Util.collectPropKeys(
      this.request.boxes,
      this.request.metrics,
      constraint.prop,
      out
    );
    return out;
  }

  putMetric(v: Native.Real<Box.Bare<C, this>>): Native.Real<Box.Bare<C, this>> {
    const index = this.request.metrics.length;
    this.request.metrics.push(v);
    return {
      type: "metricRef",
      index,
    };
  }

  pushConstraint(c: Native.Constraint<Box.Bare<C, this>>): void {
    this.request.constraints.push(c);
  }

  solve(): "unknown" | "unsat" | SolveResult<C> {
    console.log(JSON.stringify(this.request));
    const res = Native.rawSolve(this.request);
    if (res === "unknown" || res === "unsat") return res;

    const result: SolveResult<C> = {
      unsat: [],
      boxes: new Map(),
    };

    for (const box of res.boxes) {
      result.boxes.set(box.id, box);
    }

    for (let i = 0; i < res.sat.length; i++) {
      if (!res.sat[i]) {
        result.unsat.push(this.request.constraints[i]);
      }
    }

    return result;
  }
}
