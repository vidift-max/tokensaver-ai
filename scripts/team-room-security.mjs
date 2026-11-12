const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "::1"]);

export function normalizeTeamRoomBaseUrl(value, { allowHttpLoopback = false } = {}) {
  if (typeof value !== "string" || value.length === 0 || value.trim() !== value) {
    throw new TypeError("Team Room API URL is required");
  }

  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new TypeError("Team Room API URL must be absolute");
  }

  const hostname = parsed.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  const loopback = LOOPBACK_HOSTS.has(hostname);
  if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && loopback && allowHttpLoopback)) {
    throw new TypeError("Team Room API must use HTTPS except explicit loopback development");
  }
  if (parsed.username || parsed.password || parsed.href !== `${parsed.origin}/`) {
    throw new TypeError("Team Room API URL cannot contain credentials, path, query or fragment");
  }

  return parsed.origin;
}

export function resolveTeamRoomRequestUrl(baseUrl, route, options = {}) {
  if (typeof route !== "string" || !route.startsWith("/") || route.startsWith("//") ||
      route.includes("\\") || route.includes("?") || route.includes("#") ||
      /[\u0000-\u001f\u007f]/.test(route)) {
    throw new TypeError("Team Room API route must be an absolute path on the configured origin");
  }

  let decoded;
  try {
    decoded = decodeURIComponent(route);
  } catch {
    throw new TypeError("Team Room API route contains invalid encoding");
  }
  if (decoded.split("/").some(segment => segment === "." || segment === "..")) {
    throw new TypeError("Team Room API route cannot traverse paths");
  }

  const base = new URL(normalizeTeamRoomBaseUrl(baseUrl, options));
  const target = new URL(route, base);
  if (target.origin !== base.origin || target.username || target.password || target.search || target.hash) {
    throw new TypeError("Team Room API route escaped the configured origin");
  }
  return target.toString();
}
