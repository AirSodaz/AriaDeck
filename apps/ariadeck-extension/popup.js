// Popup: per-site cookie opt-in and the last send result.
//
// The only state it owns is the cookie allow-list. Everything else about a
// download is decided at send time in background.js.

import {
  ERROR_TEXT,
  cookieOrigins,
  cookiesAllowedFor,
  lastResult,
  originOf,
  requestCookieAccess,
  revokeCookieAccess,
} from "./bridge.js";

const originLabel = document.getElementById("origin");
const cookieState = document.getElementById("cookie-state");
const cookieToggle = document.getElementById("cookie-toggle");
const allowedBlock = document.getElementById("allowed-block");
const allowedList = document.getElementById("allowed");
const lastLine = document.getElementById("last");

async function activeOrigin() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return originOf(tab?.url ?? "");
}

async function render() {
  const origin = await activeOrigin();

  if (origin) {
    originLabel.textContent = new URL(origin).host;
    const allowed = await cookiesAllowedFor(origin);
    cookieState.textContent = allowed ? "Enabled" : "Off";
    cookieState.className = `status ${allowed ? "ok" : ""}`;
    cookieToggle.hidden = false;
    cookieToggle.textContent = allowed ? "Turn off" : "Enable";
    cookieToggle.className = allowed ? "" : "primary";
    cookieToggle.onclick = async () => {
      cookieToggle.disabled = true;
      try {
        if (allowed) {
          await revokeCookieAccess(origin);
        } else if (!(await requestCookieAccess(origin))) {
          // The user declined the permission prompt; nothing was stored.
          cookieState.textContent = "Permission declined";
          cookieState.className = "status bad";
          return;
        }
        await render();
      } finally {
        cookieToggle.disabled = false;
      }
    };
  } else {
    // No http(s) origin — an internal page, a PDF viewer, a file:// URL.
    originLabel.textContent = "this page";
    cookieState.textContent = "Not available here";
    cookieState.className = "status";
    cookieToggle.hidden = true;
  }

  const origins = await cookieOrigins();
  allowedBlock.hidden = origins.length === 0;
  allowedList.replaceChildren(
    ...origins.map((entry) => {
      const item = document.createElement("li");
      item.textContent = new URL(entry).host;
      return item;
    }),
  );

  const result = await lastResult();
  if (!result) {
    lastLine.textContent = "";
    lastLine.className = "status";
  } else if (result.ok) {
    const count = result.accepted ?? 1;
    lastLine.textContent = `Last send: ${count} handed to AriaDeck.`;
    lastLine.className = "status ok";
  } else {
    lastLine.textContent = `Last send: ${ERROR_TEXT[result.error] ?? "failed."}`;
    lastLine.className = "status bad";
  }
}

void render();
