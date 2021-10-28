export interface KeyInfo {
  type: KeyType;
  data: string;
}

export type KeyType = "base64Secret" | "rsaPem" | "ecPem";

export interface Validation {
  leeway: number;
  validate_exp?: boolean;
  validate_nbf?: boolean;
  aud?: string[];
  iss?: string;
  sub?: string;
  algorithms: Algorithm[];
}

export interface Header {
  typ?: "JWT";
  alg: Algorithm;
  cty?: string;
  jku?: string;
  kid?: string;
  x5u?: string;
  x5t?: string;
}

export type Algorithm =
  | "HS256"
  | "HS384"
  | "HS512"
  | "ES256"
  | "ES384"
  | "RS256"
  | "RS384"
  | "RS512"
  | "PS256"
  | "PS384"
  | "PS512";

export function encode(header: Header, claims: unknown, key: KeyInfo): string {
  return <string>(
    __blueboat_host_invoke("crypto_jwt_encode", header, claims, key)
  );
}

export function decode(
  token: string,
  key: KeyInfo,
  validation: Validation
): { header: Header; claims: unknown } {
  return <any>(
    __blueboat_host_invoke("crypto_jwt_decode", token, key, validation)
  );
}
