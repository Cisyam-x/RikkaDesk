import * as React from "react";

import { CheckCircle2, KeyRound, Loader2, Trash2 } from "lucide-react";
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
import api, { ApiError } from "~/services/api";

const DEFAULT_PROVIDER_ID = "rikkadesk-openai-compatible";
const DEFAULT_PROVIDER_NAME = "OpenAI Compatible";

interface DesktopProviderModelConfig {
  id: string;
  modelId: string;
  displayName: string;
}

interface DesktopProviderResponse {
  id: string;
  type: "openai-compatible";
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

function emptyForm(): ProviderFormState {
  return {
    id: DEFAULT_PROVIDER_ID,
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

function safeErrorMessage(error: unknown): string {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }
  return "Provider settings request failed.";
}

export function ProviderSettingsDialog({ open, onOpenChange }: ProviderSettingsDialogProps) {
  const [form, setForm] = React.useState<ProviderFormState>(() => emptyForm());
  const [loading, setLoading] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [clearingSecret, setClearingSecret] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const updateForm = React.useCallback(
    (field: keyof ProviderFormState, value: string | boolean) => {
      setForm((current) => ({
        ...current,
        [field]: value,
      }));
    },
    [],
  );

  const loadProvider = React.useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const providers = await api.get<DesktopProviderResponse[]>("desktop/providers");
      const provider = providers.find((item) => item.type === "openai-compatible") ?? providers[0];
      setForm(provider ? formFromProvider(provider) : emptyForm());
    } catch (loadError) {
      setError(safeErrorMessage(loadError));
    } finally {
      setLoading(false);
    }
  }, []);

  React.useEffect(() => {
    if (!open) return;
    void loadProvider();
  }, [loadProvider, open]);

  const handleSave = React.useCallback(async () => {
    const id = form.id.trim() || DEFAULT_PROVIDER_ID;
    const name = form.name.trim() || DEFAULT_PROVIDER_NAME;
    const baseUrl = form.baseUrl.trim();
    const modelId = form.modelId.trim();
    const displayName = form.displayName.trim() || modelId;
    const apiKey = form.apiKey.trim();

    if (!baseUrl) {
      setError("Base URL is required.");
      return;
    }
    if (!modelId) {
      setError("Model ID is required.");
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const provider = await api.post<DesktopProviderResponse>("desktop/providers", {
        id,
        type: "openai-compatible",
        enabled: true,
        name,
        baseUrl,
        modelId,
        displayName,
      });

      let hasSecret = provider.hasSecret;
      if (apiKey) {
        const secretResponse = await api.post<{ status: string; hasSecret: boolean }>(
          `desktop/providers/${provider.id}/secret`,
          { apiKey },
        );
        hasSecret = secretResponse.hasSecret;
      }

      setForm({
        id: provider.id,
        name: provider.name,
        baseUrl: provider.baseUrl,
        modelId: provider.model.modelId,
        displayName: provider.model.displayName,
        apiKey: "",
        hasSecret,
      });
      toast.success("Provider settings saved.");
    } catch (saveError) {
      setError(safeErrorMessage(saveError));
    } finally {
      setSaving(false);
    }
  }, [form]);

  const handleClearSecret = React.useCallback(async () => {
    if (!form.id.trim() || clearingSecret) return;

    setClearingSecret(true);
    setError(null);
    try {
      await api.delete<{ status: string; hasSecret: boolean }>(
        `desktop/providers/${form.id}/secret`,
      );
      setForm((current) => ({ ...current, apiKey: "", hasSecret: false }));
      toast.success("API key cleared.");
    } catch (clearError) {
      setError(safeErrorMessage(clearError));
    } finally {
      setClearingSecret(false);
    }
  }, [clearingSecret, form.id]);

  const busy = loading || saving || clearingSecret;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="size-5" />
            Provider Settings
          </DialogTitle>
          <DialogDescription>OpenAI-compatible</DialogDescription>
        </DialogHeader>

        <div className="space-y-5">
          <div className="flex items-center justify-between rounded-md border px-3 py-2">
            <div className="min-w-0">
              <div className="text-sm font-medium">Secret status</div>
              <div className="mt-0.5 truncate text-xs text-muted-foreground">
                {form.hasSecret ? "Stored in the desktop secret store" : "No API key saved"}
              </div>
            </div>
            <Badge variant={form.hasSecret ? "secondary" : "outline"} className="gap-1">
              {form.hasSecret ? <CheckCircle2 className="size-3" /> : null}
              {form.hasSecret ? "hasSecret: true" : "hasSecret: false"}
            </Badge>
          </div>

          {error && (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <div className="grid gap-4 sm:grid-cols-2">
            <label className="space-y-1.5 text-sm font-medium">
              <span>Provider Name</span>
              <Input
                value={form.name}
                onChange={(event) => updateForm("name", event.target.value)}
                placeholder={DEFAULT_PROVIDER_NAME}
                disabled={busy}
              />
            </label>

            <label className="space-y-1.5 text-sm font-medium">
              <span>Display Name</span>
              <Input
                value={form.displayName}
                onChange={(event) => updateForm("displayName", event.target.value)}
                placeholder={form.modelId || "gpt-4o-mini"}
                disabled={busy}
              />
            </label>
          </div>

          <label className="space-y-1.5 text-sm font-medium">
            <span>Base URL</span>
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
            <span>Model ID</span>
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
            <span>API Key</span>
            <Input
              value={form.apiKey}
              onChange={(event) => updateForm("apiKey", event.target.value)}
              type="password"
              placeholder={form.hasSecret ? "Leave blank to keep saved key" : "Optional"}
              autoComplete="off"
              spellCheck={false}
              disabled={busy}
            />
          </label>
        </div>

        <DialogFooter className="gap-2 sm:justify-between">
          <Button
            type="button"
            variant="outline"
            onClick={() => void handleClearSecret()}
            disabled={busy || !form.hasSecret}
          >
            {clearingSecret ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Trash2 className="size-4" />
            )}
            Clear API Key
          </Button>
          <Button type="button" onClick={() => void handleSave()} disabled={busy}>
            {saving ? <Loader2 className="size-4 animate-spin" /> : null}
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
