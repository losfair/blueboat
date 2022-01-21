import { JTDSchemaType } from "../../jsland/dist/src/validation/jtd";

interface TestData {
  a: number;
  b: number;
}

const schema_TestData: JTDSchemaType<TestData> = {
  properties: {
    a: { type: "uint32" },
    b: { type: "uint32" }
  },
};
const validator_TestData = new Validation.JTD.JTDStaticSchema(schema_TestData);

Router.post("/validate", async req => {
  let body: unknown = await req.json();
  if(!validator_TestData.validate(body)) {
    return new Response(validator_TestData.lastError!, {
      status: 400,
    });
  }

  return new Response(JSON.stringify({
    valueA: body.a,
    valueB: body.b,
  }), {
    headers: {
      "Content-Type": "application/json",
    },
  });
})

Router.get("/validate/dynschema", async req => {
  const validator = new Validation.JTD.JTDStaticSchema(schema_TestData);
  if(!validator.validate({})) {
    return new Response(validator.lastError!, {
      status: 400,
    });
  }

  return new Response(JSON.stringify({}), {
    headers: {
      "Content-Type": "application/json",
    },
  });
})