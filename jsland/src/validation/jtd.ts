import { JTDSchemaType } from "ajv/dist/core";
import { HostObject } from "../host_object";
export { JTDSchemaType } from "ajv/dist/core";

export class JTDStaticSchema<T> extends HostObject {
  lastError: string | undefined;

  constructor(schema: JTDSchemaType<T>) {
    const sym = <symbol>__blueboat_host_invoke("jtd_load_schema", schema);
    super(sym);
  }

  validate(input: unknown): input is T {
    this.lastError = <string>(
      __blueboat_host_invoke("jtd_validate", this.hostSymbol, input)
    );
    return this.lastError === undefined;
  }
}
