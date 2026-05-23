/**
 * parseExecuteTags — unit tests (Playwright runner, no browser needed).
 *
 * Vitest is not installed in this project; these assertions run in the
 * Playwright node environment via a trivial assert helper. All cases are
 * pure-function: no browser, no DOM, no network.
 *
 * Cases:
 *   1. No tags → single prose block.
 *   2. One tag in the middle → prose + execute + prose.
 *   3. Multiple tags → interleaved blocks in order.
 *   4. Unclosed tag → rendered as prose (safety net).
 *   5. Tag-only input → single execute block, no surrounding prose.
 *   6. Empty string → single empty prose block.
 */

import { test, expect } from "@playwright/test";
import { parseExecuteTags } from "../src/features/cc-bridge/parseExecuteTags";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function prose(text: string) {
  return { kind: "prose" as const, text };
}
function execute(cmd: string) {
  return { kind: "execute" as const, cmd };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test("no tags → single prose block", () => {
  const result = parseExecuteTags("Just some explanation text.");
  expect(result).toEqual([prose("Just some explanation text.")]);
});

test("one tag → prose + execute + prose", () => {
  const result = parseExecuteTags(
    "Try this:\n<execute>echo HELLO</execute>\nand that's it."
  );
  expect(result).toEqual([
    prose("Try this:\n"),
    execute("echo HELLO"),
    prose("\nand that's it."),
  ]);
});

test("multiple tags → interleaved blocks in order", () => {
  const result = parseExecuteTags(
    "First:\n<execute>echo A</execute>\nThen:\n<execute>ls -la</execute>\nDone."
  );
  expect(result).toEqual([
    prose("First:\n"),
    execute("echo A"),
    prose("\nThen:\n"),
    execute("ls -la"),
    prose("\nDone."),
  ]);
});

test("unclosed tag → rendered as prose", () => {
  const result = parseExecuteTags("Watch out: <execute>incomplete");
  // The text from <execute> onward is prose (including the tag itself)
  expect(result).toEqual([
    prose("Watch out: "),
    prose("<execute>incomplete"),
  ]);
});

test("tag-only input → single execute block, no extra prose", () => {
  const result = parseExecuteTags("<execute>ls -la</execute>");
  expect(result).toEqual([execute("ls -la")]);
});

test("empty string → single empty prose block", () => {
  const result = parseExecuteTags("");
  expect(result).toEqual([prose("")]);
});

test("whitespace is trimmed inside execute tags", () => {
  const result = parseExecuteTags("<execute>  echo hi  </execute>");
  expect(result).toEqual([execute("echo hi")]);
});
