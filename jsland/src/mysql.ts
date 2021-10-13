import { BlueboatBootstrapData } from "./native_schema";
import { wrapNativeAsync } from "./util";

export interface Mysql {
  exec<Spec extends string>(
    stmt: string,
    args: Record<string, MysqlInputType>,
    outSpec: Spec
  ): Promise<Row<Spec>[]>;
  startTransaction(): Promise<void>;
  commit(): Promise<void>;
  rollback(): Promise<void>;
}

type ValueSpec = "i" | "I" | "f" | "s" | "b" | "d";
type RowSpec<TThis extends ValueSpec, TRem extends string> = `${TThis}${TRem}`;

// https://github.com/microsoft/TypeScript/issues/23182#issuecomment-379091887
type Fin<T, U> = [U] extends [never]
  ? never
  : U extends void
  ? [T]
  : U extends unknown[]
  ? [T, ...U]
  : [T, U];

// Compute the type of the value list for a row.
type Row<TRow> = TRow extends RowSpec<infer TThis, infer TRem>
  ? TThis extends "i"
    ? Fin<number | null, Row<TRem>>
    : TThis extends "I"
    ? Fin<bigint | null, Row<TRem>>
    : TThis extends "f"
    ? Fin<number | null, Row<TRem>>
    : TThis extends "s"
    ? Fin<string | null, Row<TRem>>
    : TThis extends "b"
    ? Fin<Uint8Array | null, Row<TRem>>
    : TThis extends "d"
    ? Fin<Date | null, Row<TRem>>
    : never
  : TRow extends ""
  ? void
  : never;

type MysqlInputType =
  | ["i", number | null | undefined]
  | ["I", bigint | null | undefined]
  | ["f", number | null | undefined]
  | ["s", string | null | undefined]
  | ["b", Uint8Array | null | undefined]
  | ["d", Date | null | undefined];

class MysqlImpl implements Mysql {
  private key: string;
  constructor(key: string) {
    this.key = key;
  }

  exec<Spec extends string>(
    stmt: string,
    args: Record<string, MysqlInputType>,
    outSpec: Spec
  ): Promise<Row<Spec>[]> {
    return wrapNativeAsync((callback) =>
      __blueboat_host_invoke(
        "mysql_exec",
        this.key,
        stmt,
        args,
        outSpec,
        callback
      )
    );
  }

  startTransaction(): Promise<void> {
    return wrapNativeAsync((callback) =>
      __blueboat_host_invoke("mysql_start_transaction", this.key, callback)
    );
  }

  private endTransaction(commit: boolean): Promise<void> {
    return wrapNativeAsync((callback) =>
      __blueboat_host_invoke(
        "mysql_end_transaction",
        this.key,
        commit,
        callback
      )
    );
  }

  commit(): Promise<void> {
    return this.endTransaction(true);
  }

  rollback(): Promise<void> {
    return this.endTransaction(false);
  }
}

export const mysql: Record<string, Mysql> = {};

export function init(bs: BlueboatBootstrapData) {
  for (const x of bs.mysql) {
    mysql[x] = new MysqlImpl(x);
  }
}
