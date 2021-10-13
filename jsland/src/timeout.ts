let nextTimeoutId = 1;
let timeoutSet: Set<number> = new Set();
let finReg: FinalizationRegistry<number> | null = null;

function ensureRegistry(): FinalizationRegistry<number> {
  if (!finReg)
    finReg = new FinalizationRegistry((id) => {
      timeoutSet.delete(id);
    });
  return finReg;
}

export function setTimeout(cb: () => unknown, ms: number): number {
  let id = nextTimeoutId++;
  timeoutSet.add(id);

  let nativeCb = () => {
    if (!timeoutSet.delete(id)) return;
    cb();
  };
  ensureRegistry().register(nativeCb, id);
  __blueboat_host_invoke("sleep", ms, nativeCb);
  return id;
}

export function clearTimeout(id: number) {
  timeoutSet.delete(id);
}

export function setInterval(): number {
  return nextTimeoutId++;
}

export function clearInterval(id: number) {
  timeoutSet.delete(id);
}
