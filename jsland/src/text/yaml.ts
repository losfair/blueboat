export function parse(x: string): unknown {
  return __blueboat_host_invoke("text_yaml_parse", x);
}

type StringifyOutput<T> = T extends undefined ? undefined : string;

export function stringify<T>(x: T): StringifyOutput<T> {
  return <StringifyOutput<T>>__blueboat_host_invoke("text_yaml_stringify", x);
}
