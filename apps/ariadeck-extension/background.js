// AriaDeck browser bridge — MV3 service worker.
//
// v1 is opt-in per download: the user picks a link or a media element and sends
// it. There is deliberately no `downloads` permission and no
// onDeterminingFilename hook, so the extension cannot take over every browser
// download until that gets its own UX pass (docs/browser-bridge.md §9).

import {
  ERROR_TEXT,
  cookieHeaderFor,
  cookiesAllowedFor,
  originOf,
  recordResult,
  sendToHost,
} from "./bridge.js";

const MENU_ID = "ariadeck-send";

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: MENU_ID,
      title: "Download with AriaDeck",
      contexts: ["link", "image", "video", "audio"],
    });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== MENU_ID) return;
  // The link target wins; for a media element the src is what the user meant.
  const url = info.linkUrl || info.srcUrl;
  void send(url, info.pageUrl || tab?.url);
});

async function send(url, pageUrl) {
  const origin = originOf(url);
  if (!origin) {
    await report({ ok: false, error: "rejected" });
    return;
  }

  const item = { url };

  // Referer is what most gated downloads actually need, and it is not a secret,
  // so it goes by default (§5).
  if (pageUrl && originOf(pageUrl)) {
    item.referer = pageUrl;
  }
  // Sending the browser's own UA keeps the engine from looking like a different
  // client to servers that check.
  item.user_agent = navigator.userAgent;

  const filename = guessFilename(url);
  if (filename) {
    // Display hint only. AriaDeck shows it and then discards it: the engine
    // resolves the real name (D-001).
    item.filename = filename;
  }

  if (await cookiesAllowedFor(origin)) {
    const cookie = await cookieHeaderFor(url);
    if (cookie) item.cookie = cookie;
  }

  await report(await sendToHost([item]));
}

/** Last path segment, when it looks like a name rather than a route. */
function guessFilename(url) {
  try {
    const path = new URL(url).pathname;
    const candidate = decodeURIComponent(path.split("/").pop() ?? "");
    // The host refuses anything path-like anyway; not sending it is tidier.
    return candidate && !candidate.includes("\\") ? candidate : null;
  } catch {
    return null;
  }
}

/**
 * Surface the outcome on the toolbar badge and keep the detail for the popup.
 *
 * The badge is used rather than a notification so the extension does not need
 * the `notifications` permission for something this small.
 */
async function report(result) {
  await recordResult({ ...result, at: Date.now() });
  if (result.ok) {
    await badge("✓", "#2e7d32", `Sent ${result.accepted} to AriaDeck`);
  } else {
    await badge("!", "#c62828", ERROR_TEXT[result.error] ?? "Send failed");
  }
}

async function badge(text, color, title) {
  await chrome.action.setBadgeText({ text });
  await chrome.action.setBadgeBackgroundColor({ color });
  await chrome.action.setTitle({ title: `AriaDeck — ${title}` });
  // Clear after a moment so a stale marker is not mistaken for a new result.
  // A service worker can be torn down before this fires; the popup still shows
  // the recorded outcome, so the badge is a hint, not the record.
  setTimeout(() => {
    void chrome.action.setBadgeText({ text: "" });
    void chrome.action.setTitle({ title: "AriaDeck" });
  }, 5000);
}
