import * as React from "react";
import { ImageOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  getManagedFileIdFromMetadata,
  getManagedFileMime,
  isManagedRasterImageMime,
  resolveManagedFileUrlAsync,
} from "~/lib/files";

interface ImagePartProps {
  url: string;
  metadata?: Record<string, unknown> | null;
}

export function ImagePart({ url, metadata }: ImagePartProps) {
  const { t } = useTranslation("message");
  const [error, setError] = React.useState(false);
  const [loaded, setLoaded] = React.useState(false);
  const [resolving, setResolving] = React.useState(false);
  const [imageUrl, setImageUrl] = React.useState<string | null>(null);
  const fileId = getManagedFileIdFromMetadata(metadata);
  const mime = getManagedFileMime(metadata);
  const canResolveImage = fileId != null && isManagedRasterImageMime(mime);

  React.useEffect(() => {
    let cancelled = false;

    setError(false);
    setLoaded(false);

    if (!canResolveImage) {
      setResolving(false);
      setImageUrl(null);
      return () => {
        cancelled = true;
      };
    }

    setResolving(true);
    void resolveManagedFileUrlAsync(url, fileId)
      .then((resolvedUrl) => {
        if (cancelled) return;
        setImageUrl(resolvedUrl);
      })
      .catch(() => {
        if (cancelled) return;
        setImageUrl(null);
      })
      .finally(() => {
        if (cancelled) return;
        setResolving(false);
      });

    return () => {
      cancelled = true;
    };
  }, [canResolveImage, fileId, url]);

  if (!url) return null;

  if (resolving) {
    return (
      <div className="relative my-2 max-w-md">
        <div className="flex h-48 items-center justify-center rounded-md border border-muted bg-muted/30">
          <div className="text-sm text-muted-foreground">Loading image...</div>
        </div>
      </div>
    );
  }

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
