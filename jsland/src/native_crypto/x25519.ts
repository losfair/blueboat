import { PublicKey as Ed25519PublicKey } from "./ed25519";

const secretKeySym = Symbol("x25519-secret-key");
const publicKeySym = Symbol("x25519-public-key");

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

  exportSecret(): Uint8Array {
    return Uint8Array.from(this[secretKeySym]);
  }

  diffieHellman(theirPublic: PublicKey): Uint8Array {
    return <Uint8Array>(
      __blueboat_host_invoke(
        "crypto_x25519_diffie_hellman",
        this[secretKeySym],
        theirPublic[publicKeySym]
      )
    );
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
          "crypto_x25519_derive_public",
          source[secretKeySym]
        )
      );
    } else {
      throw new Error("invalid public key");
    }
  }

  convertToEd25519(): Ed25519PublicKey {
    const bytes = <Uint8Array>(
      __blueboat_host_invoke(
        "crypto_x25519_pubkey_to_ed25519",
        this[publicKeySym]
      )
    );
    return new Ed25519PublicKey(bytes);
  }

  exportPublic(): Uint8Array {
    return Uint8Array.from(this[publicKeySym]);
  }
}
