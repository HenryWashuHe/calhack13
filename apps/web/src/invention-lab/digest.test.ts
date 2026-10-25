// Eric's Claude: the synchronous digest against published SHA-256 vectors and
// genesis-core's canonical JSON ordering.
import { expect, it } from "vitest";
import { canonicalJson, canonicalSha256, sha256Hex } from "./digest";

const enc = (s: string) => new TextEncoder().encode(s);

it("matches the FIPS 180-2 SHA-256 test vectors, including a two-block message", () => {
  expect(sha256Hex(enc(""))).toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
  expect(sha256Hex(enc("abc"))).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  expect(sha256Hex(enc("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"))).toBe(
    "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
  );
  expect(sha256Hex(new Uint8Array(1_000_000).fill(0x61))).toBe("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
});

it("sorts keys at every level, keeps array order and writes no whitespace", () => {
  expect(canonicalJson({ b: [3, { y: 1, x: "é/\n" }], a: null, c: true })).toBe('{"a":null,"b":[3,{"x":"é/\\n","y":1}],"c":true}');
  expect(canonicalSha256({ b: 1, a: 2 })).toBe(sha256Hex(enc('{"a":2,"b":1}')));
});
