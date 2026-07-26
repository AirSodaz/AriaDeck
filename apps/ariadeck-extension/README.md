# ariadeck-extension

Reference Chrome/Edge extension (MV3) for the AriaDeck browser bridge.
Normative spec: [`docs/browser-bridge.md`](../../docs/browser-bridge.md) (**D-045**).

It hands a download to a running AriaDeck through the native messaging host
[`ariadeck-bridge`](../ariadeck-bridge/README.md), together with the headers a
gated download actually needs. It is write-only: it learns nothing about
AriaDeck's tasks, settings, or paths — only whether the hand-off was accepted.

## What v1 does, and does not

**Does:** right-click a link, image, video, or audio element → *Download with
AriaDeck*. One download at a time, chosen by the user.

**Does not:** take over browser downloads. There is no `downloads` permission
and no `onDeterminingFilename` hook, so nothing is intercepted that the user did
not explicitly send. Blanket interception needs its own UX pass and is deferred
(spec §9).

## Permissions, and why each one

| Permission | Why | When |
| --- | --- | --- |
| `contextMenus` | The one entry point | install |
| `nativeMessaging` | Talk to `ariadeck-bridge` | install |
| `storage` | Remember which sites you enabled cookies for | install |
| `activeTab` | Let the popup name the site you are on. Grants access to one tab, only while you are using the extension — not to every tab | install |
| `cookies` | Read the site's cookie for a gated download | **optional** — only if you enable cookies |
| host access (`*://*/*`) | Cookies are readable per origin | **optional**, requested one origin at a time |

Two things the manifest deliberately does *not* declare:

- **No `externally_connectable`.** A web page must not be able to reach this
  extension, and therefore must not be able to reach the host. Required by §7.
- **No blanket host permissions at install.** `optional_host_permissions` means
  the extension starts with access to no site, and gains one origin only when you
  turn cookies on for it.

## Cookies

Off everywhere by default. Enabling them for a site takes **three** independent
decisions, and all three are required:

1. This extension's popup → *Enable* for that site (allow-lists the origin).
2. The browser's permission prompt (grants read access to that origin only).
3. AriaDeck → Settings → System → *Send cookies*.

Only cookies matching the exact download URL are sent — `chrome.cookies.getAll({
url })` does the domain and path matching, so another site's cookies cannot ride
along. If a stored allow-list entry loses its browser permission, no cookie is
sent.

On the AriaDeck side the cookie stays in memory for that one add: never written
to `settings.json`, `history.sqlite`, logs, or a diagnostic export, and never
restored after a restart (§5).

## Loading it unpacked

1. Install and register the host — see
   [`ariadeck-bridge`](../ariadeck-bridge/README.md). Registration needs the
   extension ID, which you only get in step 3, so expect to do this twice the
   first time.
2. `chrome://extensions` → enable *Developer mode* → *Load unpacked* → this
   directory. (Edge: `edge://extensions`.)
3. Copy the extension ID Chrome assigns, then:
   `ariadeck-bridge --register --extension-id <ID>`
4. Reload the extension. Start AriaDeck, right-click a link, choose *Download
   with AriaDeck*.

The toolbar badge shows the outcome briefly; the popup keeps the last result,
including why a send failed.

## Verifying the contract end to end

The point of the bridge is that a download needing a `Referer` or a cookie
succeeds through it and fails without it. To check that:

1. Copy a download URL from a site that gates on `Referer` (many CDNs do) and
   paste it into AriaDeck's Add Download dialog by hand → it should fail, 403.
2. Send the same download via the context menu → it should succeed, because the
   page URL travelled as `referer`.
3. For a login-gated file, repeat with cookies off, then on.

## Tests

```sh
node --test "apps/ariadeck-extension/*.test.js"
```

They cover the URL rules and the payload limits — the item cap and the 64 KiB
message ceiling — because those numbers also exist on the host side and a silent
drift between the two would turn a clean local refusal into a `too_large` reply.
`chrome.*` paths need a browser, so anything expressing a contract bound stays in
a plain function that these can reach.

## Before store submission

- **Icons.** No `action.default_icon` / `icons` block yet, so the toolbar shows
  the default puzzle piece. The stores want 16/32/48/128 px PNGs; render them from
  `apps/ariadeck-desktop/assets/icon.svg`.
- **Pin the ID.** Build the host with `ARIADECK_EXTENSION_ID` set to the published
  ID so the installer can offer its opt-in task.
- **Version.** `manifest.json` carries its own version; it is not read from
  `Cargo.toml`.
- **Exclude the dev files** from the zip: `package.json`, `*.test.js`, and this
  README. The browser ignores them; the store listing does not need them.

Firefox is not a first-party target: MV3 and native-messaging manifests differ
enough to warrant a fork (§9).
