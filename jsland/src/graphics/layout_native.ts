export interface Request<K> {
  boxes: K[];
  metrics: Real<K>[];
  constraints: Constraint<K>[];
}

export type LayoutBoxProperty = "left" | "top" | "bottom" | "right" | "z";

export type Real<K> =
  | {
      type: "boxProperty";
      id_index: number;
      property: LayoutBoxProperty;
    }
  | {
      type: "const";
      value: number;
    }
  | {
      type: "add";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "sub";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "mul";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "div";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "select";
      cond: Prop<K>;
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "metricRef";
      index: number;
    };

export type Prop<K> =
  | {
      type: "eq";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "lt";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "le";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "gt";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "ge";
      left: Real<K>;
      right: Real<K>;
    }
  | {
      type: "and";
      left: Prop<K>;
      right: Prop<K>;
    }
  | {
      type: "or";
      left: Prop<K>;
      right: Prop<K>;
    }
  | {
      type: "not";
      value: Prop<K>;
    };

export interface Constraint<K> {
  prop: Prop<K>;
  weight: number;
}

export interface Solution<K> {
  boxes: BoxSolution<K>[];
  sat: boolean[];
}

export interface BoxSolution<K> {
  id: K;
  left: number;
  top: number;
  bottom: number;
  right: number;
  z: number;
}

export function rawSolve<K>(
  req: Request<K>
): Solution<K> | "unknown" | "unsat" {
  return <any>__blueboat_host_invoke("graphics_layout_solve", req);
}
