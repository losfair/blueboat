export type NativeAsyncCallback<T> = (
  err: Error | undefined,
  result: T | undefined
) => void;

export async function wrapNativeAsync<T>(
  f: (callback: NativeAsyncCallback<T>) => void
): Promise<T> {
  let nativeRejection = false;
  try {
    return await new Promise((resolve, reject) => {
      f((err, res) => {
        if (err) {
          nativeRejection = true;
          reject(err);
        } else {
          resolve(res!);
        }
      });
    });
  } catch (e) {
    if (e instanceof Error && nativeRejection) {
      try {
        throw new Error();
      } catch (that) {
        e.stack = that.stack;
      }
    }
    throw e;
  }
}
