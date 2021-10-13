import * as Native from "./layout_native";
import * as Util from "./layout_util";

export type HorizontalAxis = "left" | "middle" | "right";
export type VerticalAxis = "top" | "middle" | "bottom";

export interface Context<C> {
  putMetric(v: Native.Real<Bare<C, this>>): Native.Real<Bare<C, this>>;
  pushConstraint(c: Native.Constraint<Bare<C, this>>): void;
}

export abstract class Box<C, Ctx extends Context<C>> {
  abstract left(): Native.Real<Bare<C, Ctx>>;
  abstract right(): Native.Real<Bare<C, Ctx>>;
  abstract top(): Native.Real<Bare<C, Ctx>>;
  abstract bottom(): Native.Real<Bare<C, Ctx>>;
  abstract minZ(): Native.Real<Bare<C, Ctx>>;
  abstract maxZ(): Native.Real<Bare<C, Ctx>>;
  abstract toJSON(): unknown;

  horizontalAxis(axis: HorizontalAxis) {
    if (axis == "left") return this.left();
    else if (axis == "middle") return Util.realAvg(this.left(), this.right());
    else if (axis == "right") return this.right();
    else throw new Error("invalid axis");
  }

  verticalAxis(axis: VerticalAxis) {
    if (axis == "top") return this.top();
    else if (axis == "middle") return Util.realAvg(this.top(), this.bottom());
    else if (axis == "bottom") return this.bottom();
    else throw new Error("invalid axis");
  }
}

export class Combined<C, Ctx extends Context<C>> extends Box<C, Ctx> {
  solver: Ctx;
  children: Box<C, Ctx>[];

  _left: Native.Real<Bare<C, Ctx>>;
  _right: Native.Real<Bare<C, Ctx>>;
  _top: Native.Real<Bare<C, Ctx>>;
  _bottom: Native.Real<Bare<C, Ctx>>;
  _minZ: Native.Real<Bare<C, Ctx>>;
  _maxZ: Native.Real<Bare<C, Ctx>>;

  constructor(solver: Ctx, children: Box<C, Ctx>[]) {
    super();

    if (!children.length) throw new Error("CombinedBox: empty children");
    this.solver = solver;
    this.children = children;

    const minLeft = children
      .map((x) => x.left())
      .reduce((a, b) => Util.realMinMax(a, b, "lt"));
    const minTop = children
      .map((x) => x.top())
      .reduce((a, b) => Util.realMinMax(a, b, "lt"));
    const maxRight = children
      .map((x) => x.right())
      .reduce((a, b) => Util.realMinMax(a, b, "gt"));
    const maxBottom = children
      .map((x) => x.bottom())
      .reduce((a, b) => Util.realMinMax(a, b, "gt"));
    const minZ = children
      .map((x) => x.minZ())
      .reduce((a, b) => Util.realMinMax(a, b, "lt"));
    const maxZ = children
      .map((x) => x.maxZ())
      .reduce((a, b) => Util.realMinMax(a, b, "gt"));
    this._left = solver.putMetric(minLeft);
    this._top = solver.putMetric(minTop);
    this._right = solver.putMetric(maxRight);
    this._bottom = solver.putMetric(maxBottom);
    this._minZ = solver.putMetric(minZ);
    this._maxZ = solver.putMetric(maxZ);
  }

  left(): Native.Real<Bare<C, Ctx>> {
    return this._left;
  }

  right(): Native.Real<Bare<C, Ctx>> {
    return this._right;
  }

  top(): Native.Real<Bare<C, Ctx>> {
    return this._top;
  }

  bottom(): Native.Real<Bare<C, Ctx>> {
    return this._bottom;
  }

  maxZ(): Native.Real<Bare<C, Ctx>> {
    return this._maxZ;
  }

  minZ(): Native.Real<Bare<C, Ctx>> {
    return this._minZ;
  }

  toJSON(): unknown {
    return this.children.map((x) => x.toJSON());
  }

  horizontallyAlign(axis: VerticalAxis, weight: number = 1) {
    genericAlign(this.solver, this.children, (l, r) => ({
      prop: {
        type: "eq",
        left: l.verticalAxis(axis),
        right: r.verticalAxis(axis),
      },
      weight: weight,
    }));
  }

  verticallyAlign(axis: HorizontalAxis, weight: number = 1) {
    genericAlign(this.solver, this.children, (l, r) => ({
      prop: {
        type: "eq",
        left: l.horizontalAxis(axis),
        right: r.horizontalAxis(axis),
      },
      weight: weight,
    }));
  }
}

export class Bare<C, Ctx extends Context<C>> extends Box<C, Ctx> {
  solver: Ctx;
  component: C;
  index: number;

  constructor(solver: Ctx, index: number, component: C) {
    super();
    this.solver = solver;
    this.component = component;
    this.index = index;
  }

  private boxProperty(
    property: Native.LayoutBoxProperty
  ): Native.Real<Bare<C, Ctx>> {
    return {
      type: "boxProperty",
      id_index: this.index,
      property,
    };
  }

  toJSON(): unknown {
    return this.component;
  }

  left(): Native.Real<Bare<C, Ctx>> {
    return this.boxProperty("left");
  }

  right(): Native.Real<Bare<C, Ctx>> {
    return this.boxProperty("right");
  }

  top(): Native.Real<Bare<C, Ctx>> {
    return this.boxProperty("top");
  }

  bottom(): Native.Real<Bare<C, Ctx>> {
    return this.boxProperty("bottom");
  }

  maxZ(): Native.Real<Bare<C, Ctx>> {
    return this.boxProperty("z");
  }

  minZ(): Native.Real<Bare<C, Ctx>> {
    return this.boxProperty("z");
  }
}

function genericAlign<C, Ctx extends Context<C>>(
  ctx: Ctx,
  boxes: Box<C, Ctx>[],
  builder: (
    left: Box<C, Ctx>,
    right: Box<C, Ctx>
  ) => Native.Constraint<Bare<C, Ctx>>
) {
  if (boxes.length) {
    for (let i = 1; i < boxes.length; i++) {
      const left = boxes[i - 1];
      const right = boxes[i];
      ctx.pushConstraint(builder(left, right));
    }
  }
}
