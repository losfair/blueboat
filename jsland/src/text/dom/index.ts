import { HostObject } from "../../host_object";

export interface HtmlParseOptions {
  fragment?: boolean,
}

export class GenericDOM extends HostObject {
  protected constructor(raw: symbol) {
    super(raw);
  }
}

export class HtmlDOM extends GenericDOM {
  constructor(text: string | Uint8Array, opts?: HtmlParseOptions) {
    const sym = <symbol>__blueboat_host_invoke("text_dom_html_parse", text, opts);
    super(sym);
  }

  serialize(): Uint8Array {
    return <Uint8Array>__blueboat_host_invoke("text_dom_html_serialize", this.hostSymbol);
  }
}

export class XmlDOM extends GenericDOM {
  constructor(text: string | Uint8Array) {
    const sym = <symbol>__blueboat_host_invoke("text_dom_xml_parse", text);
    super(sym);
  }

  serialize(): Uint8Array {
    return <Uint8Array>__blueboat_host_invoke("text_dom_xml_serialize", this.hostSymbol);
  }
}