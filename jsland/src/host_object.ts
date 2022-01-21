// Make this lazy to play well with snapshots
let finReg: FinalizationRegistry<symbol> | null = null;

function removeHostObject(sym: symbol) {
  __blueboat_host_invoke("host_object_remove", sym);
}

function getFinReg(): FinalizationRegistry<symbol> {
  if (finReg === null) {
    finReg = new FinalizationRegistry<symbol>(heldValue => {
      removeHostObject(heldValue);
    });
  }
  return finReg;
}

export abstract class HostObject {
  constructor(public readonly hostSymbol: symbol) {
    getFinReg().register(this, this.hostSymbol);
  }

  destroy(): void {
    removeHostObject(this.hostSymbol);
  }
}
