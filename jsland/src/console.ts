export interface Console {
  log(...args: unknown[]): void;
  warn(...args: unknown[]): void;
  error(...args: unknown[]): void;
}

export const console: Console = {
  log(...args: unknown[]): void {
    __blueboat_host_invoke("log", ...args);
  },
  warn(...args: unknown[]): void {
    __blueboat_host_invoke("log", ...args);
  },
  error(...args: unknown[]): void {
    __blueboat_host_invoke("log", ...args);
  },
};
