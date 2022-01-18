import {
  S3Credentials,
  S3ListObjectsV2Output,
  S3ListObjectsV2Request,
  S3PresignInfo,
  S3PresignOptions,
  S3Region,
} from "../../native_schema";
import { wrapNativeAsync } from "../../util";

export interface JsAwsCredentials {
  key: string;
  secret: string;
}

export interface AwsSignaturePayloadBase {
  method: string;
  service: string;
  region: AwsRegion;
  path: string;
  headers: Record<string, string[]>;
}

export interface AwsSignaturePayload extends AwsSignaturePayloadBase {
  presignedUrl?: false;
  body?: string | Uint8Array | null | undefined;
}

export interface AwsSignaturePayloadPresignedUrl extends AwsSignaturePayloadBase {
  expiresInMillis: number;
  presignedUrl: true;
}

export interface AwsSignatureOutput {
  headers: Record<string, string[]>,
  url: string,
}

export type SigningOutput<T> = T extends AwsSignaturePayloadPresignedUrl ? string : T extends AwsSignaturePayload ? AwsSignatureOutput : never;

export interface AwsRegion {
  name: string;
  endpoint?: string | null | undefined;
}

export function sign<T extends AwsSignaturePayload | AwsSignaturePayloadPresignedUrl>(
  creds: JsAwsCredentials,
  payload: T,
): SigningOutput<T> {
  return <SigningOutput<T>>(
    __blueboat_host_invoke(
      "external_aws_sign",
      creds,
      payload,
    )
  );
}

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
