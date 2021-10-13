/// <reference path="../jsland/dist/src/index.d.ts" />

Router.post("/log", async req => {
  const body = await req.json();
  if(!body || typeof body.apppath != "string" || typeof body.limit != "number") {
    return new Response("bad request", { status: 400 });
  }

  const apppath = <string>body.apppath;
  const limit = <number>body.limit;

  const rows = await App.mysql.sys.exec(`select appversion, reqid, msg, logseq, logtime from applog where apppath = :apppath order by logtime desc limit :limit;`, {
    apppath: ['s', apppath],
    limit: ['i', limit],
  }, "sssid");
  const ret = rows.map(([appversion, reqid, msg, logseq, logtime]) => ({
    appversion,
    reqid,
    msg,
    logseq,
    logtime,
  }));
  let res = new Response(JSON.stringify(ret), {
    headers: {
      "Content-Type": "application/json",
    },
  });
  return res;
});

