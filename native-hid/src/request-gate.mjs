// The same gate as the Rust API (src/api.rs): only a loopback Host, so a
// DNS-rebinding page cannot reach this server, and only these origins, so a
// random web page cannot drive the HID WebSocket. Browsers do not apply CORS
// to WebSockets, which is why the upgrade checks Origin itself.
const ALLOWED_ORIGINS = new Set([
  'https://control.openmouse.app',
  'https://dev.openmouse.app',
  'https://openmouse.app',
  'https://www.openmouse.app',
  // This server is a development tool; the control panel's Vite dev server.
  'http://localhost:5173',
  'http://127.0.0.1:5173',
]);

export function loopbackHost(host) {
  const match = /^(localhost|127\.0\.0\.1|\[::1\])(?::(\d{1,5}))?$/.exec(host ?? '');
  if (!match) return false;
  return match[2] === undefined || (Number(match[2]) > 0 && Number(match[2]) <= 65535);
}

/** Whether a request may be served: loopback Host, and an allowed Origin if any. */
export function requestAllowed(headers, { requireOrigin = false } = {}) {
  if (!loopbackHost(headers.host)) return false;
  const origin = headers.origin;
  if (origin === undefined) return !requireOrigin;
  return ALLOWED_ORIGINS.has(origin);
}
