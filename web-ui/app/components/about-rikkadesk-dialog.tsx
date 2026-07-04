import { Info } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "~/components/ui/dialog";

const RIKKADESK_VERSION = "0.1.0";
const RIKKADESK_FEATURE_TAG = "rikkadesk-v0.1.0-beta.8";

const SUPPORT_KEYS = [
  "openai_streaming",
  "conversation_management",
  "provider_settings",
  "test_connection",
  "provider_import_export",
] as const;

const UNSUPPORTED_KEYS = [
  "files",
  "media",
  "search",
  "tools",
  "workspace",
  "multimodal",
] as const;

const DOC_PATHS = [
  "docs/rikkadesk-beta4-release-notes.md",
  "docs/rikkadesk-beta-package-checklist.md",
  "CHANGELOG.md",
];

interface AboutRikkaDeskDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function AboutRikkaDeskDialog({ open, onOpenChange }: AboutRikkaDeskDialogProps) {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[92svh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Info className="size-5" />
            {t("about_rikkadesk.title")}
          </DialogTitle>
          <DialogDescription>{t("about_rikkadesk.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-5 text-sm">
          <section className="rounded-lg border bg-muted/30 p-4">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="text-base font-semibold text-foreground">RikkaDesk</h3>
              <Badge variant="secondary">{t("about_rikkadesk.private_beta")}</Badge>
              <Badge variant="outline">{t("about_rikkadesk.unsigned_installer")}</Badge>
            </div>
            <dl className="mt-4 grid gap-3 sm:grid-cols-2">
              <div>
                <dt className="text-xs font-medium text-muted-foreground">
                  {t("about_rikkadesk.version_label")}
                </dt>
                <dd className="mt-1 font-mono text-foreground">{RIKKADESK_VERSION}</dd>
              </div>
              <div>
                <dt className="text-xs font-medium text-muted-foreground">
                  {t("about_rikkadesk.feature_tag_label")}
                </dt>
                <dd className="mt-1 break-all font-mono text-foreground">
                  {RIKKADESK_FEATURE_TAG}
                </dd>
              </div>
            </dl>
          </section>

          <div className="grid gap-4 sm:grid-cols-2">
            <section className="rounded-lg border p-4">
              <h3 className="font-medium text-foreground">{t("about_rikkadesk.support_title")}</h3>
              <ul className="mt-3 space-y-2 text-muted-foreground">
                {SUPPORT_KEYS.map((key) => (
                  <li key={key}>- {t(`about_rikkadesk.support.${key}`)}</li>
                ))}
              </ul>
            </section>

            <section className="rounded-lg border p-4">
              <h3 className="font-medium text-foreground">
                {t("about_rikkadesk.unsupported_title")}
              </h3>
              <ul className="mt-3 space-y-2 text-muted-foreground">
                {UNSUPPORTED_KEYS.map((key) => (
                  <li key={key}>- {t(`about_rikkadesk.unsupported.${key}`)}</li>
                ))}
              </ul>
            </section>
          </div>

          <section className="rounded-lg border p-4">
            <h3 className="font-medium text-foreground">{t("about_rikkadesk.docs_title")}</h3>
            <p className="mt-2 text-muted-foreground">{t("about_rikkadesk.docs_description")}</p>
            <div className="mt-3 flex flex-col gap-2">
              {DOC_PATHS.map((path) => (
                <code
                  key={path}
                  className="rounded-md bg-muted px-2 py-1 text-xs text-muted-foreground"
                >
                  {path}
                </code>
              ))}
            </div>
          </section>
        </div>

        <DialogFooter>
          <Button type="button" onClick={() => onOpenChange(false)}>
            {t("about_rikkadesk.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
