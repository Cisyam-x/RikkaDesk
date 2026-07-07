import { File, FileText } from "lucide-react";

import { resolveFileUrl } from "~/lib/files";

interface DocumentPartProps {
  url: string;
  fileName: string;
  mime: string;
}

function getDocumentIcon(mime: string) {
  if (mime === "application/pdf") {
    return <FileText className="h-4 w-4" />;
  }
  return <File className="h-4 w-4" />;
}

export function DocumentPart({ url, fileName, mime }: DocumentPartProps) {
  const documentUrl = resolveFileUrl(url);
  const safeFileName = fileName?.trim() || "Attachment";
  const safeMime = mime?.trim() || "application/octet-stream";
  const content = (
    <>
      {getDocumentIcon(safeMime)}
      <span className="max-w-[320px] truncate">{safeFileName}</span>
      <span className="text-muted-foreground">{safeMime}</span>
    </>
  );

  if (!documentUrl) {
    return (
      <span className="my-2 inline-flex max-w-full items-center gap-2 rounded-full border border-muted bg-card px-3 py-1.5 text-sm text-muted-foreground">
        {content}
      </span>
    );
  }

  return (
    <a
      className="my-2 inline-flex max-w-full items-center gap-2 rounded-full border border-muted bg-card px-3 py-1.5 text-sm hover:bg-muted/40"
      href={documentUrl}
      rel="noopener noreferrer"
      target="_blank"
    >
      {content}
    </a>
  );
}
