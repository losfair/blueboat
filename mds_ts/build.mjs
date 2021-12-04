import { build } from 'esbuild'
import * as fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const files = fs.readdirSync(path.join(__dirname, './input/'));
for(const file of files) {
  if(!file.endsWith('.js')) continue;
  console.log(file);
  await build({
    entryPoints: [path.join(__dirname, './input/', file)],
    target: 'es6',
    outfile: path.join(__dirname, './output', file),
    minify: true,
  });
}
