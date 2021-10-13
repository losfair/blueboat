import * as Native from "./layout_native";

export function collectRealKeys<K>(
  boxes: K[],
  metrics: Native.Real<K>[],
  p: Native.Real<K>,
  out: [K, Native.LayoutBoxProperty][]
) {
  switch (p.type) {
    case "boxProperty":
      const x: [K, Native.LayoutBoxProperty] = [boxes[p.id_index], p.property];
      out.push(x);
      break;
    case "const":
      break;
    case "add":
    case "sub":
      collectRealKeys(boxes, metrics, p.left, out);
      collectRealKeys(boxes, metrics, p.right, out);
      break;
    case "select":
      collectPropKeys(boxes, metrics, p.cond, out);
      collectRealKeys(boxes, metrics, p.left, out);
      collectRealKeys(boxes, metrics, p.right, out);
      break;
    case "metricRef":
      collectRealKeys(boxes, metrics, metrics[p.index], out);
      break;
    default:
      throw new Error("unreachable");
  }
}

export function collectPropKeys<K>(
  boxes: K[],
  metrics: Native.Real<K>[],
  p: Native.Prop<K>,
  out: [K, Native.LayoutBoxProperty][]
) {
  switch (p.type) {
    case "eq":
    case "lt":
    case "le":
    case "gt":
    case "ge":
      collectRealKeys(boxes, metrics, p.left, out);
      collectRealKeys(boxes, metrics, p.right, out);
      break;
    case "and":
    case "or":
      collectPropKeys(boxes, metrics, p.left, out);
      collectPropKeys(boxes, metrics, p.right, out);
      break;
    case "not":
      collectPropKeys(boxes, metrics, p.value, out);
      break;
    default:
      throw new Error("unreachable");
  }
}

export function realMinMax<K>(
  a: Native.Real<K>,
  b: Native.Real<K>,
  cmp: "lt" | "gt"
): Native.Real<K> {
  return {
    type: "select",
    cond: {
      type: cmp,
      left: a,
      right: b,
    },
    left: a,
    right: b,
  };
}

export function realAvg<K>(
  a: Native.Real<K>,
  b: Native.Real<K>
): Native.Real<K> {
  return {
    type: "div",
    left: {
      type: "add",
      left: a,
      right: b,
    },
    right: {
      type: "const",
      value: 2,
    },
  };
}

export function realCmp<K>(
  op: "eq" | "lt" | "le" | "gt" | "ge",
  a: Native.Real<K>,
  b: Native.Real<K>
): Native.Prop<K> {
  return {
    type: op,
    left: a,
    right: b,
  };
}

export function propLogic<K>(
  op: "and" | "or",
  a: Native.Prop<K>,
  b: Native.Prop<K>
): Native.Prop<K> {
  return {
    type: op,
    left: a,
    right: b,
  };
}

export function propNot<K>(x: Native.Prop<K>): Native.Prop<K> {
  return {
    type: "not",
    value: x,
  };
}
