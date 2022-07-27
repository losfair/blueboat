import { HostObject } from "../../host_object";

export interface HtmlParseOptions {
  fragment?: boolean,
}

export type FilterExpr = {
  type: "and",
  left: FilterExpr,
  right: FilterExpr,
} | {
  type: "or",
  left: FilterExpr,
  right: FilterExpr,
} | {
  type: "tag",
  tag: string,
} | {
  type: "hasClass",
  className: string,
} | {
  type: "text",
} | {
  type: "true",
} | {
  type: "false",
};

export type JsNodeData = {
  type: "text",
  text: string,
} | {
  type: "element",
  name: string,
  attrs: JsElemAttr[],
} | {
  type: "other",
}

export type JsElemAttr = {
  name: string,
  value: string,
}

export class Node extends HostObject {
  protected constructor(raw: symbol) {
    super(raw);
  }

  queryWithFilter(filter: FilterExpr, callback: (node: this) => boolean) {
    __blueboat_host_invoke("text_dom_query_with_filter", this.hostSymbol, filter, (sym: symbol) => {
      const node: this = new (this.constructor as any)(sym);
      return callback(node);
    });
  }

  get(): JsNodeData {
    return <JsNodeData>__blueboat_host_invoke("text_dom_get", this.hostSymbol);
  }

  update(data: JsNodeData) {
    __blueboat_host_invoke("text_dom_update", this.hostSymbol, data);
  }

  remove(): boolean {
    return <boolean>__blueboat_host_invoke("text_dom_remove", this.hostSymbol);
  }

  appendChild(child: this): boolean {
    return <boolean>__blueboat_host_invoke("text_dom_append_child", this.hostSymbol, child.hostSymbol);
  }
}

export class HTML extends Node {
  private constructor(raw: symbol) {
    super(raw);
  }
  static parse(text: string | Uint8Array, opts?: HtmlParseOptions): HTML {
    const sym = <symbol>__blueboat_host_invoke("text_dom_html_parse", text, opts);
    return new HTML(sym);
  }

  serialize(): Uint8Array {
    return <Uint8Array>__blueboat_host_invoke("text_dom_html_serialize", this.hostSymbol);
  }
}

export class XML extends Node {
  private constructor(raw: symbol) {
    super(raw);
  }
  static parse(text: string | Uint8Array): XML {
    const sym = <symbol>__blueboat_host_invoke("text_dom_xml_parse", text);
    return new XML(sym);
  }

  serialize(): Uint8Array {
    return <Uint8Array>__blueboat_host_invoke("text_dom_xml_serialize", this.hostSymbol);
  }
}