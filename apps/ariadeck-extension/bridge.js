// Shared bridge vocabulary and state. Kept separate from background.js so the
// popup can read the same policy without duplicating it.
//
// Contract: docs/browser-bridge.md (D-045). The payload shape here is the whole
// of what the host accepts; adding a field has to happen there first.

export const HOST_NAME = "com.ariadeck.bridge";

/** Extension → host protocol version. The host rejects anything else. */
export const MESSAGE_VERSION = 1;

/** Matches the host's own cap, so an oversize batch is caught before sending. */
const MAX_MESSAGE_BYTES = 64 * 1024;

/** Matches MAX_LAUNCH_ITEMS on the AriaDeck side. */
const MAX_ITEMS = 32;

const STORAGE_KEY_COOKIE_ORIGINS = "cookieOrigins";
const STORAGE_KEY_LAST_RESULT = "lastResult";

/**
 * Human-readable text for each error code the host can return. The host
 * deliberately tells us nothing else — no paths, no settings, no task state.
 */
export const ERROR_TEXT = {
  not_running: "AriaDeck is not running. Start it and try again.",
  rejected: "AriaDeck refused this download.",
  too_large: "That batch was too large to send.",
  unsupported_version: "This extension is newer than the installed AriaDeck.",
  timeout: "AriaDeck did not answer in time.",
  host_missing:
    "The AriaDeck browser bridge is not installed. Enable it in the AriaDeck " +
    "installer, or run: ariadeck-bridge --register --extension-id <ID>",
};

/** Origin of a URL, or null when it has none we would ever send to. */
export function originOf(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:"
      ? parsed.origin
      : null;
  } catch {
    return null;
  }
}

export async function cookieOrigins() {
  const stored = await chrome.storage.local.get(STORAGE_KEY_COOKIE_ORIGINS);
  const origins = stored[STORAGE_KEY_COOKIE_ORIGINS];
  return Array.isArray(origins) ? origins : [];
}

export async function setCookieOrigins(origins) {
  await chrome.storage.local.set({
    [STORAGE_KEY_COOKIE_ORIGINS]: [...new Set(origins)].sort(),
  });
}

/**
 * Whether cookies may be attached for this origin.
 *
 * Both halves are required, per contract §5.1: the user allow-listed the origin
 * here, *and* the browser actually granted us permission to read its cookies.
 * A stale allow-list entry whose permission was revoked sends no cookie.
 */
export async function cookiesAllowedFor(origin) {
  if (!origin) return false;
  const allowed = await cookieOrigins();
  if (!allowed.includes(origin)) return false;
  return chrome.permissions.contains({
    permissions: ["cookies"],
    origins: [`${origin}/*`],
  });
}

/** Ask for the cookie permission and the single origin it applies to. */
export async function requestCookieAccess(origin) {
  const granted = await chrome.permissions.request({
    permissions: ["cookies"],
    origins: [`${origin}/*`],
  });
  if (granted) {
    await setCookieOrigins([...(await cookieOrigins()), origin]);
  }
  return granted;
}

/** Drop the origin from the allow-list and give its host permission back. */
export async function revokeCookieAccess(origin) {
  const remaining = (await cookieOrigins()).filter((entry) => entry !== origin);
  await setCookieOrigins(remaining);
  await chrome.permissions.remove({ origins: [`${origin}/*`] });
  if (remaining.length === 0) {
    // Nothing left that needs cookie reading, so hand the capability back too
    // rather than keeping a permission with no remaining purpose.
    await chrome.permissions.remove({ permissions: ["cookies"] });
  }
}

/**
 * Cookie header value for exactly this URL, or null.
 *
 * `chrome.cookies.getAll({ url })` is what keeps this origin-scoped: the browser
 * returns only cookies whose domain and path match the URL we are about to hand
 * over, so cookies from other sites cannot leak into the request.
 */
export async function cookieHeaderFor(url) {
  const cookies = await chrome.cookies.getAll({ url });
  if (!cookies.length) return null;
  const header = cookies
    .map((cookie) => `${cookie.name}=${cookie.value}`)
    .join("; ");
  // Single line only; the host refuses control characters outright.
  return /[\r\n]/.test(header) ? null : header;
}

/** Build the host payload. Fields not in the D-045 whitelist are never added. */
export function buildMessage(items) {
  return { version: MESSAGE_VERSION, items };
}

export function messageTooLarge(message) {
  return new TextEncoder().encode(JSON.stringify(message)).length >
    MAX_MESSAGE_BYTES;
}

export function batchTooLarge(items) {
  return items.length === 0 || items.length > MAX_ITEMS;
}

/**
 * Send a batch to the host and normalize the outcome.
 *
 * Resolves to `{ ok, error? , accepted? }`; it never throws, so callers do not
 * have to distinguish "host absent" from "host said no".
 */
export async function sendToHost(items) {
  if (batchTooLarge(items)) {
    return { ok: false, error: "too_large" };
  }
  const message = buildMessage(items);
  if (messageTooLarge(message)) {
    return { ok: false, error: "too_large" };
  }
  try {
    const reply = await chrome.runtime.sendNativeMessage(HOST_NAME, message);
    if (reply && reply.ok === true) {
      return { ok: true, accepted: reply.accepted ?? items.length };
    }
    return { ok: false, error: reply?.error ?? "rejected" };
  } catch {
    // connectNative throwing means the host is not registered at all, which is
    // a different fix from "AriaDeck is not running".
    return { ok: false, error: "host_missing" };
  }
}

export async function recordResult(result) {
  await chrome.storage.local.set({ [STORAGE_KEY_LAST_RESULT]: result });
}

export async function lastResult() {
  const stored = await chrome.storage.local.get(STORAGE_KEY_LAST_RESULT);
  return stored[STORAGE_KEY_LAST_RESULT] ?? null;
}
