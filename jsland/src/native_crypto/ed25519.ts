import { digest } from "./index";
import {
  PublicKey as X25519PublicKey,
  SecretKey as X25519SecretKey,
} from "./x25519";

const secretKeySym = Symbol("ed25519-secret-key");
const publicKeySym = Symbol("ed25519-public-key");

export class Keypair {
  secret: SecretKey;
  public: PublicKey;

  constructor(secretKey: SecretKey, publicKey: PublicKey) {
    if (!secretKey[secretKeySym] || !publicKey[publicKeySym])
      throw new TypeError("invalid keypair parameters");
    this.secret = secretKey;
    this.public = publicKey;
  }

  sign(message: Uint8Array): Uint8Array {
    return <Uint8Array>(
      __blueboat_host_invoke(
        "crypto_ed25519_sign",
        this.public[publicKeySym],
        this.secret[secretKeySym],
        message
      )
    );
  }
}

export class SecretKey {
  [secretKeySym]: Uint8Array;

  constructor(bytes?: Uint8Array) {
    if (!bytes) {
      bytes = crypto.getRandomValues(new Uint8Array(32));
    } else {
      bytes = Uint8Array.from(bytes);
    }

    if (bytes.byteLength != 32) {
      throw new Error("invalid secret key");
    }

    this[secretKeySym] = bytes;
  }

  convertToX25519(): X25519SecretKey {
    const secret = digest("sha512", this[secretKeySym]).subarray(0, 32);
    return new X25519SecretKey(secret);
  }

  exportSecret(): Uint8Array {
    return Uint8Array.from(this[secretKeySym]);
  }
}

export class PublicKey {
  [publicKeySym]: Uint8Array;

  constructor(source: Uint8Array | SecretKey) {
    if (source instanceof Uint8Array) {
      if (source.byteLength != 32) {
        throw new Error("invalid public key");
      }
      this[publicKeySym] = Uint8Array.from(source);
    } else if (source instanceof SecretKey) {
      this[publicKeySym] = <Uint8Array>(
        __blueboat_host_invoke(
          "crypto_ed25519_derive_public",
          source[secretKeySym]
        )
      );
    } else {
      throw new Error("invalid public key");
    }
  }

  exportPublic(): Uint8Array {
    return Uint8Array.from(this[publicKeySym]);
  }

  convertToX25519(): X25519PublicKey {
    const bytes = <Uint8Array>(
      __blueboat_host_invoke(
        "crypto_ed25519_pubkey_to_x25519",
        this[publicKeySym]
      )
    );
    return new X25519PublicKey(bytes);
  }

  verify(
    signature: Uint8Array,
    message: Uint8Array,
    strict?: boolean
  ): boolean {
    return <boolean>(
      __blueboat_host_invoke(
        "crypto_ed25519_verify",
        this[publicKeySym],
        signature,
        message,
        !!strict
      )
    );
  }
}
