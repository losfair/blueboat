#!/usr/bin/env zx

const crypto = require("crypto");
const fs = require("fs");

let tmpdir = (await $`mktemp --suffix=-blueboat-build -d`).stdout.trim();

// Just making sure we won't do something like `rm -rf /`...
if(!tmpdir.startsWith("/tmp/")) throw new Error("bad tmpdir");
process.on("exit", () => {
  fs.rmdirSync(tmpdir, { recursive: true });
});
["SIGHUP", "SIGINT", "SIGTERM"].forEach((sig) => {
  process.on(sig, () => {
    fs.rmdirSync(tmpdir, { recursive: true });
  });
});

let file = argv.f;
let s3_bucket = argv.s3_bucket;
let s3_prefix = argv.s3_prefix;
let env_file = argv.env;
let mysql_file = argv.mysql;
if(!file || !s3_bucket || !s3_prefix) throw new Error("missing args");
if(!s3_prefix.endsWith("/") || s3_prefix.startsWith("/")) throw new Error("invalid s3 prefix");

let s3_invoke = ["s3cmd"]
if(process.env.S3CMD_CFG) {
  s3_invoke.push("-c", process.env.S3CMD_CFG)
}

let packageVersion = computeFileHash(file);
const packageFilename = `${new Date().toISOString()}_${packageVersion}.tar`;
const packageS3Path = `${s3_prefix}${packageFilename}`;
console.log(`package path ${packageS3Path}`);
let md = {
  version: packageVersion,
  package: packageS3Path,
  env: {},
  mysql: {},
};

if(env_file) md.env = JSON.parse(fs.readFileSync(env_file, "utf-8"));
if(mysql_file) md.mysql = JSON.parse(fs.readFileSync(mysql_file, "utf-8"));
await $`${s3_invoke} put ${file} ${"s3://" + s3_bucket + "/" + packageS3Path}`;

let md_path = path.join(tmpdir, `metadata.json`);
fs.writeFileSync(md_path, JSON.stringify(md));
await $`${s3_invoke} put ${md_path} ${"s3://" + s3_bucket + "/" + s3_prefix + "metadata.json"}`;

function computeFileHash(filename) {
  const buf = fs.readFileSync(filename);
  const hash = crypto.createHash("sha256");
  hash.update(buf);
  return hash.digest('hex');
}
