// Run with: node --test "apps/ariadeck-extension/*.test.js"
//
// Pins the parts of manifest.json that fail quietly. A wrong icon path or size
// is accepted by Chrome and by the store: you get the default puzzle piece, or a
// rescaled blur, with no error anywhere. And the permission shape is a D-045 §7
// requirement, which until now only review enforced.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const manifest = JSON.parse(readFileSync(`${here}manifest.json`, "utf8"));

/** Width and height straight out of the PNG IHDR chunk. */
function pngSize(path) {
  const bytes = readFileSync(path);
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  assert.ok(bytes.subarray(0, 8).equals(signature), `${path} is not a PNG`);
  // 8 signature + 4 length + 4 "IHDR" = 16, then two big-endian u32.
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

test("every declared icon exists and is exactly its declared size", () => {
  const blocks = [
    ["icons", manifest.icons],
    ["action.default_icon", manifest.action.default_icon],
  ];
  for (const [label, block] of blocks) {
    assert.ok(block, `${label} is missing`);
    assert.deepEqual(
      Object.keys(block),
      ["16", "32", "48", "128"],
      `${label} must declare the four sizes Chrome and the store ask for`,
    );
    for (const [size, relative] of Object.entries(block)) {
      const expected = Number(size);
      const { width, height } = pngSize(`${here}${relative}`);
      assert.deepEqual(
        { width, height },
        { width: expected, height: expected },
        `${label}["${size}"] -> ${relative}`,
      );
    }
  }
});

test("no permission grants access to a site at install time", () => {
  // Everything reaching a site is optional and requested per origin (§7). The
  // send path needs none of it: the URLs come from the context-menu event.
  assert.deepEqual(manifest.permissions, [
    "activeTab",
    "contextMenus",
    "nativeMessaging",
    "storage",
  ]);
  assert.equal(manifest.host_permissions, undefined);
  assert.deepEqual(manifest.optional_permissions, ["cookies"]);
  assert.deepEqual(manifest.optional_host_permissions, ["*://*/*"]);
});

test("a web page cannot reach the extension, and so cannot reach the host", () => {
  // §7: externally_connectable would put the native host one message away from
  // any page allowed to connect. The context menu is the only entry point.
  assert.equal(manifest.externally_connectable, undefined);
  assert.equal(manifest.content_scripts, undefined);
  assert.equal(manifest.web_accessible_resources, undefined);
});

test("browser downloads are not intercepted", () => {
  // Blanket interception is deferred (§9); the `downloads` permission is what
  // would silently enable it.
  const all = [
    ...(manifest.permissions ?? []),
    ...(manifest.optional_permissions ?? []),
  ];
  for (const permission of ["downloads", "tabs", "webRequest", "<all_urls>"]) {
    assert.equal(all.includes(permission), false, `${permission} must not be requested`);
  }
});
