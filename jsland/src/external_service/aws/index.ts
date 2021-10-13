import {
  S3Credentials,
  S3ListObjectsV2Output,
  S3ListObjectsV2Request,
  S3PresignInfo,
  S3PresignOptions,
  S3Region,
} from "../../native_schema";
import { wrapNativeAsync } from "../../util";

export function getPresignedUrl(
  region: S3Region,
  credentials: S3Credentials,
  info: S3PresignInfo,
  options: S3PresignOptions
): string {
  return <string>(
    __blueboat_host_invoke(
      "external_s3_sign",
      region,
      credentials,
      info,
      options
    )
  );
}

export function listObjectsV2(
  region: S3Region,
  credentials: S3Credentials,
  req: S3ListObjectsV2Request
): Promise<S3ListObjectsV2Output> {
  return wrapNativeAsync((callback) =>
    __blueboat_host_invoke(
      "external_s3_list_objects_v2",
      region,
      credentials,
      req,
      callback
    )
  );
}
