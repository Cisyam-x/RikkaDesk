import { appendWebAuthQuery } from "~/services/api";

/**
 * Convert file URL to the correct API endpoint
 * - http/https URLs are returned as-is (external files)
 * - /api/files/path/{id} URLs are authenticated and returned
 * - file:, data:, and arbitrary relative paths are rejected by default
 */
export function resolveFileUrl(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return "";

  if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
    return trimmed;
  }

  const match = trimmed.match(/^\/?api\/files\/path\/(\d+)$/);
  if (match) {
    return appendWebAuthQuery(`/api/files/path/${match[1]}`);
  }

  return "";
}
