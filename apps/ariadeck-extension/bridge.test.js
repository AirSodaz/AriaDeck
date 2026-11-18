// Run with: node --test "apps/ariadeck-extension/*.test.js"
//
// Covers the parts of bridge.js that encode the D-045 limits and the URL rules.
// The chrome.* paths are not exercised here — they need a browser — so the rule
// is that anything expressing a contract bound stays in a plain function that
// can be tested from here.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MESSAGE_VERSION,
  batchTooLarge,
  buildMessage,
  guessFilename,
  hostLabelOf,
  messageTooLarge,
  originOf,
} from "./bridge.js";

const item = (index) => ({ url: `https://example.test/${index}.bin` });

test("only http and https URLs yield an origin", () => {
  assert.equal(originOf("https://example.test/a?b=c#d"), "https://example.test");
  assert.equal(originOf("http://example.test:8080/a"), "http://example.test:8080");
  for (const rejected of [
    "file:///C:/Windows/win.ini",
    "data:text/html,<script>",
    "javascript:alert(1)",
    "blob:https://example.test/abc",
    "ftp://example.test/f",
    "chrome://extensions",
    "not a url",
    "",
  ]) {
    assert.equal(originOf(rejected), null, `${rejected} must have no origin`);
  }
});

test("messages declare the protocol version the host accepts", () => {
  // The host rejects any other value with unsupported_version.
  assert.equal(MESSAGE_VERSION, 1);
  assert.deepEqual(buildMessage([item(0)]), {
    version: 1,
    items: [{ url: "https://example.test/0.bin" }],
  });
});

test("batch size matches the host's item cap", () => {
  // MAX_LAUNCH_ITEMS on the AriaDeck side is 32. Refusing locally gives a
  // clearer failure than letting the host answer too_large.
  assert.equal(batchTooLarge([]), true, "an empty batch is not worth sending");
  assert.equal(batchTooLarge(Array.from({ length: 32 }, (_, i) => item(i))), false);
  assert.equal(batchTooLarge(Array.from({ length: 33 }, (_, i) => item(i))), true);
});

test("oversize messages are caught before they reach the host", () => {
  assert.equal(messageTooLarge(buildMessage([item(0)])), false);
  // One item can exhaust the 64 KiB whole-message budget on its own.
  const huge = { url: `https://example.test/${"x".repeat(70 * 1024)}` };
  assert.equal(messageTooLarge(buildMessage([huge])), true);
});

test("byte length, not character count, decides the size limit", () => {
  // A multi-byte URL must not slip past a naive length check.
  const multibyte = { url: `https://example.test/${"下".repeat(30 * 1024)}` };
  assert.equal(multibyte.url.length < 64 * 1024, true, "under the limit in chars");
  assert.equal(
    messageTooLarge(buildMessage([multibyte])),
    true,
    "but over it in bytes",
  );
});

test("hostLabelOf extracts host and tolerates malformed storage entries", () => {
  assert.equal(hostLabelOf("https://example.test:8443"), "example.test:8443");
  assert.equal(hostLabelOf("not a valid url"), "not a valid url");
});

test("guessFilename enforces IPC filename hint rules and drops unsafe segments", () => {
  assert.equal(guessFilename("https://example.test/files/archive.tar.gz"), "archive.tar.gz");
  assert.equal(guessFilename("https://example.test/files/my%20file.zip"), "my file.zip");
  for (const dropped of [
    "https://example.test/",
    "https://example.test/files/%20%20",
    "https://example.test/files/a%2Fb.bin",
    "https://example.test/files/a%5Cb.bin",
    "https://example.test/files/%2e%2e",
    "https://example.test/files/.",
    "https://example.test/files/bad%00name.bin",
    `https://example.test/files/${"a".repeat(1025)}.bin`,
  ]) {
    assert.equal(guessFilename(dropped), null, `${dropped} must not emit a filename hint`);
  }
});
