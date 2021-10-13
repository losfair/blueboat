import { compile } from 'json-schema-to-typescript'
import * as path from "path";
import * as fs from "fs";
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const nativeSchema = JSON.parse(fs.readFileSync(path.join(__dirname, "../native_schema.json"), "utf-8"));
const ts = await compile(nativeSchema, "Native");
fs.writeFileSync(path.join(__dirname, `../src/native_schema.ts`), ts);
