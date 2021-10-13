import { JTDSchemaType } from "ajv/dist/core";
export { JTDSchemaType } from "ajv/dist/core";

const NativeKey = Symbol("native_key");

export class JTDStaticSchema<T> {
  [NativeKey]: symbol;
  lastError: string | undefined;

  constructor(schema: JTDSchemaType<T>) {
    this[NativeKey] = <symbol>__blueboat_host_invoke("jtd_load_schema", schema);
  }

  validate(input: unknown): input is T {
    this.lastError = <string>(
      __blueboat_host_invoke("jtd_validate", this[NativeKey], input)
    );
    return this.lastError === undefined;
  }
}
