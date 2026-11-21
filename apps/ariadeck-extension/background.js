// AriaDeck browser bridge — MV3 service worker.
//
// v1 is opt-in per download: the user picks a link or a media element and sends
// it. There is deliberately no `downloads` permission and no
// onDeterminingFilename hook, so the extension cannot take over every browser
// download until that gets its own UX pass (docs/browser-bridge.md §9).

import {
  ERROR_TEXT,
  MAX_REFERER_BYTES,
  MAX_USER_AGENT_BYTES,
  cookieHeaderFor,
  cookiesAllowedFor,
  guessFilename,
  originOf,
  recordResult,
  sanitizeHeaderValue,
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
  try {
    const origin = originOf(url);
    if (!origin) {
      await report({ ok: false, error: "rejected" });
      return;
    }

    const item = { url };

    // Referer is what most gated downloads actually need, and it is not a secret,
    // so it goes by default (§5).
    if (pageUrl && originOf(pageUrl)) {
      const referer = sanitizeHeaderValue(pageUrl, MAX_REFERER_BYTES);
      if (referer) item.referer = referer;
    }
    // Sending the browser's own UA keeps the engine from looking like a different
    // client to servers that check.
    const userAgent = sanitizeHeaderValue(navigator.userAgent, MAX_USER_AGENT_BYTES);
    if (userAgent) {
      item.user_agent = userAgent;
    }

    const filename = guessFilename(url);
    if (filename) {
      // Display hint only. AriaDeck shows it and then discards it: the engine
      // resolves the real name (D-001).
      item.filename = filename;
    }

    if (await cookiesAllowedFor(origin)) {
      try {
        const cookie = await cookieHeaderFor(url);
        if (cookie) item.cookie = cookie;
      } catch {
        // Optional `cookies` permission may have been revoked between check and
        // lookup; degrade gracefully to sending without cookies.
      }
    }

    await report(await sendToHost([item]));
  } catch {
    await report({ ok: false, error: "rejected" });
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

let badgeClearTimer = null;

async function badge(text, color, title) {
  if (badgeClearTimer !== null) {
    clearTimeout(badgeClearTimer);
    badgeClearTimer = null;
  }
  await chrome.action.setBadgeText({ text });
  await chrome.action.setBadgeBackgroundColor({ color });
  await chrome.action.setTitle({ title: `AriaDeck — ${title}` });
  // Clear after a moment so a stale marker is not mistaken for a new result.
  // A service worker can be torn down before this fires; the popup still shows
  // the recorded outcome, so the badge is a hint, not the record.
  badgeClearTimer = setTimeout(() => {
    badgeClearTimer = null;
    void chrome.action.setBadgeText({ text: "" });
    void chrome.action.setTitle({ title: "AriaDeck" });
  }, 5000);
}
