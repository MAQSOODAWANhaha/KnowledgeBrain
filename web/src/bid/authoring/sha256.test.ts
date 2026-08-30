import { sha256HexSync } from "../../sha256";
import { describe, expect, it } from "./harness";

describe("sha256HexSync", () => {
  it("matches empty and abc vectors", () => {
    expect(sha256HexSync(new TextEncoder().encode(""))).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
    expect(sha256HexSync(new TextEncoder().encode("abc"))).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  });
});
