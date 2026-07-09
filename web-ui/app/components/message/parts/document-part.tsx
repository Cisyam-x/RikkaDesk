import * as React from "react";
import { File, FileText } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  formatFileSize,
  getManagedFileIdFromMetadata,
  getManagedFileMime,
  getManagedFileSizeBytes,
  resolveManagedFileUrlAsync,
} from "~/lib/files";

interface DocumentPartProps {
  url: string;
  fileName: string;
  mime: string;
  metadata?: Record<string, unknown> | null;
}

function getDocumentIcon(mime: string) {
  if (mime === "application/pdf") {
    return <FileText className="h-4 w-4" />;
  }
  return <File className="h-4 w-4" />;
}

function normalizeMime(mime: string | null | undefined): string | null {
  const value = mime?.trim();
  return value || null;
}

export function DocumentPart({ url, fileName, mime, metadata }: DocumentPartProps) {
  const { t } = useTranslation("message");
  const [documentUrl, setDocumentUrl] = React.useState<string | null>(null);
  const fileId = getManagedFileIdFromMetadata(metadata);
  const sizeBytes = getManagedFileSizeBytes(metadata);
  const metadataMime = getManagedFileMime(metadata);
  const safeFileName = fileName?.trim() || t("attachment_part.attachment");
  const safeMime = metadataMime ?? normalizeMime(mime) ?? "application/octet-stream";

  React.useEffect(() => {
    let cancelled = false;

    setDocumentUrl(null);
    void resolveManagedFileUrlAsync(url, fileId)
      .then((resolvedUrl) => {
        if (cancelled) return;
        setDocumentUrl(resolvedUrl);
      })
      .catch(() => {
        if (cancelled) return;
        setDocumentUrl(null);
      });

    return () => {
      cancelled = true;
    };
  }, [fileId, url]);

  const content = (
    <>
      {getDocumentIcon(safeMime)}
      <span className="max-w-[320px] truncate">{safeFileName}</span>
      <span className="text-muted-foreground">{safeMime}</span>
      {sizeBytes != null ? (
        <span className="text-muted-foreground">{formatFileSize(sizeBytes)}</span>
      ) : null}
    </>
  );

  if (!documentUrl) {
    return (
      <span
        aria-disabled="true"
        className="my-2 inline-flex max-w-full items-center gap-2 rounded-full border border-muted bg-card px-3 py-1.5 text-sm text-muted-foreground"
      >
        {content}
      </span>
    );
  }

  return (
    <a
      className="my-2 inline-flex max-w-full items-center gap-2 rounded-full border border-muted bg-card px-3 py-1.5 text-sm hover:bg-muted/40"
      href={documentUrl}
      referrerPolicy="no-referrer"
      rel="noopener noreferrer"
      target="_blank"
    >
      {content}
    </a>
  );
}
