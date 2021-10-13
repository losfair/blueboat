/// <reference path="../jsland/dist/src/index.d.ts" />

Router.any("/", async req => {
  return fetch(req);
});

