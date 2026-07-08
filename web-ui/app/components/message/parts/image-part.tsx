import * as React from "react";
import { ImageOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  getManagedFileIdFromMetadata,
  getManagedFileMime,
  isManagedRasterImageMime,
  resolveManagedFileUrl,
} from "~/lib/files";

interface ImagePartProps {
  url: string;
  metadata?: Record<string, unknown> | null;
}

export function ImagePart({ url, metadata }: ImagePartProps) {
  const { t } = useTranslation("message");
  const [error, setError] = React.useState(false);
  const [loaded, setLoaded] = React.useState(false);
  const fileId = getManagedFileIdFromMetadata(metadata);
  const mime = getManagedFileMime(metadata);
  const imageUrl = fileId != null && isManagedRasterImageMime(mime)
    ? resolveManagedFileUrl(url, fileId)
    : null;

  if (!url) return null;

  if (!imageUrl || error) {
    return (
      <div className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        <ImageOff className="h-4 w-4" />
        <span>
          {error
            ? t("attachment_part.image_unavailable")
            : t("attachment_part.image_blocked")}
        </span>
      </div>
    );
  }

  return (
    <div className="relative my-2 max-w-md">
      {!loaded && (
        <div className="flex h-48 items-center justify-center rounded-md border border-muted bg-muted/30">
          <div className="text-sm text-muted-foreground">Loading image...</div>
        </div>
      )}
      <img
        src={imageUrl}
        alt={t("attachment_part.image_alt")}
        className={`rounded-md border border-muted object-contain ${
          loaded ? "block" : "absolute left-0 top-0 h-px w-px opacity-0"
        }`}
        decoding="async"
        loading="lazy"
        onLoad={() => setLoaded(true)}
        onError={() => setError(true)}
        referrerPolicy="no-referrer"
        style={{ maxHeight: "500px", width: "auto" }}
      />
    </div>
  );
}
