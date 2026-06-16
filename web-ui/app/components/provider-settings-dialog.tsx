import * as React from "react";

import { CheckCircle2, KeyRound, Loader2, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

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
import { Input } from "~/components/ui/input";
import { ScrollArea } from "~/components/ui/scroll-area";
import api, { ApiError } from "~/services/api";
import { cn } from "~/lib/utils";

const DEFAULT_PROVIDER_NAME = "OpenAI Compatible";
const PROVIDER_TYPE = "openai-compatible";

interface DesktopProviderModelConfig {
  id: string;
  modelId: string;
  displayName: string;
}

interface DesktopProviderResponse {
  id: string;
  type: typeof PROVIDER_TYPE;
  enabled: boolean;
  name: string;
  baseUrl: string;
  model: DesktopProviderModelConfig;
  secretRef: string;
  hasSecret: boolean;
}

interface ProviderFormState {
  id: string;
  name: string;
  baseUrl: string;
  modelId: string;
  displayName: string;
  apiKey: string;
  hasSecret: boolean;
}

interface ProviderSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function createProviderId(): string {
  return `provider-openai-compatible-${Date.now()}`;
}

function emptyForm(id = createProviderId()): ProviderFormState {
  return {
    id,
    name: DEFAULT_PROVIDER_NAME,
    baseUrl: "",
    modelId: "",
    displayName: "",
    apiKey: "",
    hasSecret: false,
  };
}

function formFromProvider(provider: DesktopProviderResponse): ProviderFormState {
  return {
    id: provider.id,
    name: provider.name || DEFAULT_PROVIDER_NAME,
    baseUrl: provider.baseUrl,
    modelId: provider.model.modelId,
    displayName: provider.model.displayName,
    apiKey: "",
    hasSecret: provider.hasSecret,
  };
}

function safeErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }
  return fallback;
}

function providerModelLabel(provider: DesktopProviderResponse): string {
  const displayName = provider.model.displayName.trim();
  return displayName || provider.model.modelId;
}

export function ProviderSettingsDialog({ open, onOpenChange }: ProviderSettingsDialogProps) {
  const { t } = useTranslation();
  const [providers, setProviders] = React.useState<DesktopProviderResponse[]>([]);
  const [selectedProviderId, setSelectedProviderId] = React.useState<string | null>(null);
  const [form, setForm] = React.useState<ProviderFormState>(() => emptyForm());
  const [loading, setLoading] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);
  const [clearingSecret, setClearingSecret] = React.useState(false);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const isExistingProvider = React.useMemo(
    () => providers.some((provider) => provider.id === form.id),
    [form.id, providers],
  );
  const busy = loading || saving || deleting || clearingSecret;

  const selectProvider = React.useCallback((provider: DesktopProviderResponse) => {
    setSelectedProviderId(provider.id);
    setForm(formFromProvider(provider));
    setError(null);
  }, []);

  const loadProviders = React.useCallback(async (preferredProviderId?: string | null) => {
    setLoading(true);
    setError(null);
    try {
      const nextProviders = await api.get<DesktopProviderResponse[]>("desktop/providers");
      setProviders(nextProviders);

      const nextProvider =
        nextProviders.find((provider) => provider.id === preferredProviderId)
        ?? nextProviders[0]
        ?? null;
      if (nextProvider) {
        setSelectedProviderId(nextProvider.id);
        setForm(formFromProvider(nextProvider));
      } else {
        setSelectedProviderId(null);
        setForm(emptyForm());
      }
    } catch (loadError) {
      setError(safeErrorMessage(loadError, t("provider_settings.request_failed")));
    } finally {
      setLoading(false);
    }
  }, [t]);

  React.useEffect(() => {
    if (!open) return;
    void loadProviders(selectedProviderId);
  }, [loadProviders, open]);

  const updateForm = React.useCallback(
    (field: keyof ProviderFormState, value: string | boolean) => {
      setForm((current) => ({
        ...current,
        [field]: value,
      }));
    },
    [],
  );

  const handleAddProvider = React.useCallback(() => {
    setSelectedProviderId(null);
    setForm(emptyForm());
    setError(null);
  }, []);

  const handleSave = React.useCallback(async () => {
    const id = form.id.trim() || createProviderId();
    const name = form.name.trim() || DEFAULT_PROVIDER_NAME;
    const baseUrl = form.baseUrl.trim();
    const modelId = form.modelId.trim();
    const displayName = form.displayName.trim() || modelId;
    const apiKey = form.apiKey.trim();

    if (!baseUrl) {
      setError(t("provider_settings.base_url_required"));
      return;
    }
    if (!modelId) {
      setError(t("provider_settings.model_id_required"));
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const provider = await api.post<DesktopProviderResponse>("desktop/providers", {
        id,
        type: PROVIDER_TYPE,
        enabled: true,
        name,
        baseUrl,
        modelId,
        displayName,
      });

      if (apiKey) {
        await api.post<{ status: string; hasSecret: boolean }>(
          `desktop/providers/${provider.id}/secret`,
          { apiKey },
        );
      }

      await loadProviders(provider.id);
      toast.success(t("provider_settings.saved"));
    } catch (saveError) {
      setError(safeErrorMessage(saveError, t("provider_settings.request_failed")));
    } finally {
      setSaving(false);
    }
  }, [form, loadProviders, t]);

  const handleClearSecret = React.useCallback(async () => {
    if (!form.id.trim() || clearingSecret || !isExistingProvider) return;

    setClearingSecret(true);
    setError(null);
    try {
      await api.delete<{ status: string; hasSecret: boolean }>(
        `desktop/providers/${form.id}/secret`,
      );
      await loadProviders(form.id);
      toast.success(t("provider_settings.api_key_cleared"));
    } catch (clearError) {
      setError(safeErrorMessage(clearError, t("provider_settings.request_failed")));
    } finally {
      setClearingSecret(false);
    }
  }, [clearingSecret, form.id, isExistingProvider, loadProviders, t]);

  const handleDeleteProvider = React.useCallback(async () => {
    if (!isExistingProvider || deleting) return;

    setDeleting(true);
    setError(null);
    try {
      await api.delete<{ status: string }>(`desktop/providers/${form.id}`);
      setDeleteConfirmOpen(false);
      toast.success(t("provider_settings.deleted"));
      await loadProviders(null);
    } catch (deleteError) {
      setError(safeErrorMessage(deleteError, t("provider_settings.request_failed")));
    } finally {
      setDeleting(false);
    }
  }, [deleting, form.id, isExistingProvider, loadProviders, t]);

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="max-h-[92svh] overflow-y-auto sm:max-w-5xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <KeyRound className="size-5" />
              {t("provider_settings.title")}
            </DialogTitle>
            <DialogDescription>
              {t("provider_settings.description")}
            </DialogDescription>
          </DialogHeader>

          <div className="grid min-h-[32rem] gap-4 lg:grid-cols-[18rem_1fr]">
            <div className="flex min-h-0 flex-col rounded-md border">
              <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
                <div>
                  <div className="text-sm font-medium">{t("provider_settings.providers")}</div>
                  <div className="text-xs text-muted-foreground">
                    {t("provider_settings.configured_count", { count: providers.length })}
                  </div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleAddProvider}
                  disabled={busy}
                >
                  <Plus className="size-4" />
                  {t("provider_settings.add_provider")}
                </Button>
              </div>

              <ScrollArea className="min-h-0 flex-1">
                <div className="space-y-2 p-2">
                  {loading ? (
                    <div className="flex items-center justify-center gap-2 rounded-md border border-dashed px-3 py-8 text-sm text-muted-foreground">
                      <Loader2 className="size-4 animate-spin" />
                      {t("provider_settings.loading_providers")}
                    </div>
                  ) : providers.length === 0 ? (
                    <div className="rounded-md border border-dashed px-3 py-8 text-center text-sm text-muted-foreground">
                      {t("provider_settings.no_providers")}
                    </div>
                  ) : (
                    providers.map((provider) => {
                      const selected = provider.id === selectedProviderId;
                      return (
                        <button
                          key={provider.id}
                          type="button"
                          className={cn(
                            "w-full rounded-md border px-3 py-2 text-left transition hover:bg-muted/60",
                            selected && "border-primary bg-primary/5",
                          )}
                          disabled={busy}
                          onClick={() => selectProvider(provider)}
                        >
                          <div className="flex items-start justify-between gap-2">
                            <div className="min-w-0">
                              <div className="truncate text-sm font-medium">{provider.name}</div>
                              <div className="mt-0.5 text-[11px] text-muted-foreground">
                                {provider.type}
                              </div>
                            </div>
                            <Badge
                              variant={provider.hasSecret ? "secondary" : "outline"}
                              className="shrink-0"
                            >
                              {provider.hasSecret
                                ? t("provider_settings.has_secret_short")
                                : t("provider_settings.no_key")}
                            </Badge>
                          </div>
                          <div className="mt-2 truncate text-xs text-muted-foreground">
                            {provider.baseUrl}
                          </div>
                          <div className="mt-1 truncate text-xs">
                            {providerModelLabel(provider)}
                            <span className="text-muted-foreground"> / {provider.model.modelId}</span>
                          </div>
                        </button>
                      );
                    })
                  )}
                </div>
              </ScrollArea>
            </div>

            <div className="min-w-0 space-y-5 rounded-md border p-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <div className="text-sm font-medium">
                    {isExistingProvider
                      ? t("provider_settings.edit_provider")
                      : t("provider_settings.new_provider")}
                  </div>
                  <div className="mt-1 max-w-xl text-xs text-muted-foreground">
                    {t("provider_settings.secret_storage_note")}
                  </div>
                </div>
                <Badge variant={form.hasSecret ? "secondary" : "outline"} className="gap-1">
                  {form.hasSecret ? <CheckCircle2 className="size-3" /> : null}
                  {form.hasSecret
                    ? t("provider_settings.has_secret_true")
                    : t("provider_settings.has_secret_false")}
                </Badge>
              </div>

              {error && (
                <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                  {error}
                </div>
              )}

              <div className="grid gap-4 sm:grid-cols-2">
                <label className="space-y-1.5 text-sm font-medium">
                  <span>{t("provider_settings.provider_name")}</span>
                  <Input
                    value={form.name}
                    onChange={(event) => updateForm("name", event.target.value)}
                    placeholder={DEFAULT_PROVIDER_NAME}
                    disabled={busy}
                  />
                </label>

                <label className="space-y-1.5 text-sm font-medium">
                  <span>{t("provider_settings.display_name")}</span>
                  <Input
                    value={form.displayName}
                    onChange={(event) => updateForm("displayName", event.target.value)}
                    placeholder={form.modelId || "gpt-4o-mini"}
                    disabled={busy}
                  />
                </label>
              </div>

              <label className="space-y-1.5 text-sm font-medium">
                <span>{t("provider_settings.base_url")}</span>
                <Input
                  value={form.baseUrl}
                  onChange={(event) => updateForm("baseUrl", event.target.value)}
                  placeholder="https://api.openai.com/v1"
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                />
              </label>

              <label className="space-y-1.5 text-sm font-medium">
                <span>{t("provider_settings.model_id")}</span>
                <Input
                  value={form.modelId}
                  onChange={(event) => updateForm("modelId", event.target.value)}
                  placeholder="gpt-4o-mini"
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                />
              </label>

              <label className="space-y-1.5 text-sm font-medium">
                <span>{t("provider_settings.api_key")}</span>
                <Input
                  value={form.apiKey}
                  onChange={(event) => updateForm("apiKey", event.target.value)}
                  type="password"
                  placeholder={form.hasSecret
                    ? t("provider_settings.api_key_keep_placeholder")
                    : t("provider_settings.api_key_optional_placeholder")}
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                />
              </label>

              <DialogFooter className="mt-5 flex flex-col gap-3 border-t pt-4 sm:flex-row sm:items-center sm:justify-between">
                <div className="flex flex-col gap-2.5 sm:flex-row sm:flex-wrap">
                  <Button
                    type="button"
                    variant="outline"
                    className="whitespace-nowrap"
                    onClick={() => void handleClearSecret()}
                    disabled={busy || !form.hasSecret || !isExistingProvider}
                  >
                    {clearingSecret ? (
                      <Loader2 className="size-4 animate-spin" />
                    ) : (
                      <Trash2 className="size-4" />
                    )}
                    {t("provider_settings.clear_api_key")}
                  </Button>
                  <Button
                    type="button"
                    variant="destructive"
                    className="whitespace-nowrap"
                    onClick={() => setDeleteConfirmOpen(true)}
                    disabled={busy || !isExistingProvider}
                  >
                    <Trash2 className="size-4" />
                    {t("provider_settings.delete_provider")}
                  </Button>
                </div>

                <Button
                  type="button"
                  className="sm:ml-auto"
                  onClick={() => void handleSave()}
                  disabled={busy}
                >
                  {saving ? <Loader2 className="size-4 animate-spin" /> : null}
                  {t("provider_settings.save")}
                </Button>
              </DialogFooter>
            </div>
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={deleteConfirmOpen} onOpenChange={setDeleteConfirmOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("provider_settings.delete_provider_title")}</DialogTitle>
            <DialogDescription>
              {t("provider_settings.delete_provider_description")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => setDeleteConfirmOpen(false)}
              disabled={deleting}
            >
              {t("provider_settings.cancel")}
            </Button>
            <Button
              type="button"
              variant="destructive"
              onClick={() => void handleDeleteProvider()}
              disabled={deleting}
            >
              {deleting ? <Loader2 className="size-4 animate-spin" /> : null}
              {t("provider_settings.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
