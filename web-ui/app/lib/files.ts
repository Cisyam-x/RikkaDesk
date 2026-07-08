import { appendWebAuthQuery, resolveApiUrl } from "~/services/api";
import type { UIMessagePart } from "~/types";

const MANAGED_FILE_PATH_PATTERN = /^\/api\/files\/path\/([1-9]\d*)$/;
const RASTER_IMAGE_MIMES = new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
]);

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

export async function resolveFileUrlAsync(url: string): Promise<string> {
  const resolved = resolveFileUrl(url);
  if (!resolved) return "";
  return resolveApiUrl(resolved);
}

function safePositiveInteger(value: unknown): number | null {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value <= 0
  ) {
    return null;
  }

  return value;
}

function managedFilePathId(url: string | undefined): number | null {
  const trimmed = url?.trim();
  if (!trimmed) return null;

  const match = trimmed.match(MANAGED_FILE_PATH_PATTERN);
  if (!match) return null;

  const id = Number(match[1]);
  return safePositiveInteger(id);
}

export function getManagedFileId(part: UIMessagePart): number | null {
  return getManagedFileIdFromMetadata(part.metadata);
}

export function getManagedFileIdFromMetadata(
  metadata: Record<string, unknown> | null | undefined,
): number | null {
  return safePositiveInteger(metadata?.fileId);
}

export function getManagedFileMime(
  metadata: Record<string, unknown> | null | undefined,
): string | null {
  const value = metadata?.mime;
  if (typeof value !== "string") return null;

  const mime = value.trim().toLowerCase();
  return mime || null;
}

export function getManagedFileSizeBytes(
  metadata: Record<string, unknown> | null | undefined,
): number | null {
  const value = metadata?.sizeBytes;
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : null;
}

export function isManagedFilePath(url: string | undefined): boolean {
  return managedFilePathId(url) != null;
}

export function isManagedRasterImageMime(mime: string | null | undefined): boolean {
  return !!mime && RASTER_IMAGE_MIMES.has(mime.trim().toLowerCase());
}

export function resolveManagedFileUrl(
  url: string | undefined,
  fileId?: number | null,
): string | null {
  const pathId = managedFilePathId(url);
  if (pathId == null) return null;

  if (fileId != null && pathId !== fileId) {
    return null;
  }

  return appendWebAuthQuery(`/api/files/path/${pathId}`);
}

export async function resolveManagedFileUrlAsync(
  url: string | undefined,
  fileId?: number | null,
): Promise<string | null> {
  const resolved = resolveManagedFileUrl(url, fileId);
  if (!resolved) return null;
  return resolveApiUrl(resolved);
}

export function formatFileSize(size: number): string {
  if (size < 1024) return `${size} B`;
  const kb = size / 1024;
  if (kb < 1024) return `${kb.toFixed(kb >= 10 ? 0 : 1)} KB`;
  const mb = kb / 1024;
  return `${mb.toFixed(mb >= 10 ? 0 : 1)} MB`;
}
