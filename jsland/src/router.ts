export interface Router {
  get(path: string, cb: HttpHandler): void;
  post(path: string, cb: HttpHandler): void;
  put(path: string, cb: HttpHandler): void;
  patch(path: string, cb: HttpHandler): void;
  delete(path: string, cb: HttpHandler): void;
  options(path: string, cb: HttpHandler): void;
  any(path: string, cb: HttpHandler): void;
  use(path: string, cb: MiddlewareHandler): void;
}

export type HttpHandler = (req: Request) => Response | Promise<Response>;
export type MiddlewareHandler = (
  req: Request,
  inner: (req: Request) => Response | Promise<Response>
) => Response | Promise<Response>;

export interface Route {
  get?: HttpHandler;
  post?: HttpHandler;
  put?: HttpHandler;
  patch?: HttpHandler;
  delete?: HttpHandler;
  options?: HttpHandler;
}

interface RouteNode {
  target: Route;
  children: Map<string, RouteNode>;
  middlewares: MiddlewareHandler[];
}

export class RouterImpl implements Router {
  private root: RouteNode = {
    target: {},
    children: new Map(),
    middlewares: [],
  };

  constructor() {}

  private createChild(
    path: string,
    creator: (r: Route, m: MiddlewareHandler[]) => void
  ) {
    if (!path.startsWith("/")) {
      throw new Error("path must start with '/'");
    }
    const segs = path.split("/");
    let n = this.root;
    for (let i = 1; i < segs.length; i++) {
      const seg = segs[i];
      if (seg == "" && i + 1 != segs.length)
        throw new Error("intermediate path segment must not be empty");
      let maybeN = n.children.get(seg);
      if (maybeN === undefined) {
        maybeN = { target: {}, children: new Map(), middlewares: [] };
        n.children.set(seg, maybeN);
      }
      n = maybeN;
    }
    creator(n.target, n.middlewares);
  }

  lookupChild(path: string): [Route, MiddlewareHandler[][]] | null {
    if (!path.startsWith("/")) {
      throw new Error("path must start with '/'");
    }
    const segs = path.split("/");
    let n = this.root;
    const mw: MiddlewareHandler[][] = [];
    for (let i = 1; i < segs.length; i++) {
      const seg = segs[i];
      if (seg == "" && i + 1 != segs.length)
        throw new Error("intermediate path segment must not be empty");

      // Append the directory middleware.
      let directoryN = n.children.get("");
      if (directoryN !== undefined && directoryN.middlewares.length) {
        mw.push(directoryN.middlewares);
      }

      let maybeN = n.children.get(seg);
      if (maybeN === undefined) {
        maybeN = n.children.get("*");
      }
      if (maybeN === undefined) {
        // Fall back to the directory handler, if any.
        if (directoryN !== undefined) {
          n = directoryN;
          break;
        } else {
          return null;
        }
      }

      if (maybeN.middlewares.length) {
        mw.push(maybeN.middlewares);
      }
      n = maybeN;
    }
    mw.reverse();
    return [n.target, mw];
  }

  get(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.get = cb;
    });
  }
  put(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.put = cb;
    });
  }
  post(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.post = cb;
    });
  }
  patch(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.patch = cb;
    });
  }
  delete(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.delete = cb;
    });
  }
  options(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.options = cb;
    });
  }
  use(path: string, cb: MiddlewareHandler): void {
    this.createChild(path, (r, m) => {
      m.push(cb);
    });
  }
  any(path: string, cb: HttpHandler): void {
    this.createChild(path, (r) => {
      r.get = cb;
      r.put = cb;
      r.post = cb;
      r.patch = cb;
      r.delete = cb;
      r.options = cb;
    });
  }
}

export const coreRouter: RouterImpl = new RouterImpl();
export const router: Router = coreRouter;
