import ky, { type Options, HTTPError } from "ky";

interface ErrorResponse {
  error: string;
  code: number;
}

interface WebAuthTokenResponse {
  token: string;
  expiresAt: number;
}

interface WebAuthRequiredEventDetail {
  message: string;
  code: number;
}

export class ApiError extends Error {
  code: number;

  constructor(message: string, code: number) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

const WEB_AUTH_STORAGE_KEY = "rikkahub:web-auth";
const WEB_AUTH_REQUIRED_EVENT = "rikkahub:web-auth-required";
const WEB_AUTH_EXPIRY_SKEW_MILLIS = 10_000;
const WEB_AUTH_QUERY_KEY = "access_token";
const DEFAULT_API_PREFIX = "/api";
const DESKTOP_API_BASE_COMMAND = "get_api_base_url";

type TauriRuntimeWindow = Window &
  typeof globalThis & {
    __TAURI_INTERNALS__?: unknown;
    __TAURI__?: unknown;
  };

let desktopApiBaseUrlPromise: Promise<string | null> | null = null;

function isBrowser(): boolean {
  return typeof window !== "undefined";
}

function isTauriRuntime(): boolean {
  if (!isBrowser()) return false;
  const runtimeWindow = window as TauriRuntimeWindow;
  return "__TAURI_INTERNALS__" in runtimeWindow || "__TAURI__" in runtimeWindow;
}

export function isDesktopRuntime(): boolean {
  return isTauriRuntime();
}

function normalizeApiPath(url: string): string {
  const trimmed = url.replace(/^\/+/, "");
  return trimmed.startsWith("api/") ? trimmed.slice(4) : trimmed;
}

function joinApiUrl(prefix: string, url: string): string {
  const normalizedPath = normalizeApiPath(url);
  return `${prefix.replace(/\/$/, "")}/${normalizedPath}`;
}

async function readDesktopApiBaseUrl(): Promise<string | null> {
  if (!isTauriRuntime()) return null;

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const baseUrl = await invoke<string>(DESKTOP_API_BASE_COMMAND);
    return baseUrl.replace(/\/$/, "");
  } catch {
    return null;
  }
}

async function resolveApiPrefixUrl(): Promise<string> {
  if (!isTauriRuntime()) {
    return DEFAULT_API_PREFIX;
  }

  desktopApiBaseUrlPromise ??= readDesktopApiBaseUrl();
  const desktopApiBaseUrl = await desktopApiBaseUrlPromise;
  return desktopApiBaseUrl ? `${desktopApiBaseUrl}/api` : DEFAULT_API_PREFIX;
}

export async function resolveApiUrl(url: string): Promise<string> {
  if (/^https?:\/\//i.test(url)) {
    return url;
  }

  return joinApiUrl(await resolveApiPrefixUrl(), url);
}

function readStoredWebAuth(): WebAuthTokenResponse | null {
  if (!isBrowser()) return null;
  const raw = window.localStorage.getItem(WEB_AUTH_STORAGE_KEY);
  if (!raw) return null;

  try {
    const parsed = JSON.parse(raw) as Partial<WebAuthTokenResponse>;
    if (typeof parsed.token !== "string" || typeof parsed.expiresAt !== "number") {
      return null;
    }
    return { token: parsed.token, expiresAt: parsed.expiresAt };
  } catch {
    return null;
  }
}

function isWebAuthExpired(expiresAt: number): boolean {
  return Date.now() >= expiresAt - WEB_AUTH_EXPIRY_SKEW_MILLIS;
}

function getValidWebAuthToken(): string | null {
  const auth = readStoredWebAuth();
  if (!auth) return null;
  if (isWebAuthExpired(auth.expiresAt)) {
    clearWebAuthToken();
    return null;
  }
  return auth.token;
}

function dispatchWebAuthRequired(detail: WebAuthRequiredEventDetail) {
  if (!isBrowser()) return;
  window.dispatchEvent(new CustomEvent<WebAuthRequiredEventDetail>(WEB_AUTH_REQUIRED_EVENT, { detail }));
}

const kyInstance = ky.create({
  timeout: 30000,
  hooks: {
    beforeRequest: [
      (request) => {
        const token = getValidWebAuthToken();
        if (!token || request.headers.has("Authorization")) return;
        request.headers.set("Authorization", `Bearer ${token}`);
      },
    ],
  },
});

async function handleError(error: unknown): Promise<never> {
  if (error instanceof HTTPError) {
    const { response } = error;
    let errorData: ErrorResponse | undefined;
    try {
      errorData = await response.json();
    } catch {
      // Ignore JSON parse error
    }
    const code = errorData?.code ?? response.status;
    const message = errorData?.error ?? error.message;
    const isAuthTokenEndpoint = response.url.includes("/api/auth/token");
    if (code === 401 && !isAuthTokenEndpoint) {
      clearWebAuthToken();
      dispatchWebAuthRequired({ message, code });
    }
    throw new ApiError(message, code);
  }
  throw error;
}

export function setWebAuthToken(token: string, expiresAt: number): void {
  if (!isBrowser()) return;
  window.localStorage.setItem(WEB_AUTH_STORAGE_KEY, JSON.stringify({ token, expiresAt }));
}

export function clearWebAuthToken(): void {
  if (!isBrowser()) return;
  window.localStorage.removeItem(WEB_AUTH_STORAGE_KEY);
}

export function onWebAuthRequired(
  listener: (detail: WebAuthRequiredEventDetail) => void,
): () => void {
  if (!isBrowser()) return () => {};

  const handler = (event: Event) => {
    const customEvent = event as CustomEvent<WebAuthRequiredEventDetail>;
    listener(customEvent.detail);
  };
  window.addEventListener(WEB_AUTH_REQUIRED_EVENT, handler);

  return () => {
    window.removeEventListener(WEB_AUTH_REQUIRED_EVENT, handler);
  };
}

export function appendWebAuthQuery(url: string): string {
  if (!isBrowser() || !url.startsWith("/api/")) return url;

  const token = getValidWebAuthToken();
  if (!token) return url;

  const [pathWithQuery, hash = ""] = url.split("#", 2);
  const separator = pathWithQuery.includes("?") ? "&" : "?";
  const encodedToken = encodeURIComponent(token);
  const nextPath = `${pathWithQuery}${separator}${WEB_AUTH_QUERY_KEY}=${encodedToken}`;
  return hash ? `${nextPath}#${hash}` : nextPath;
}

/**
 * API client with unwrapped response data
 */
const api = {
  async get<T>(url: string, options?: Options): Promise<T> {
    try {
      return await kyInstance.get(await resolveApiUrl(url), options).json<T>();
    } catch (error) {
      return handleError(error);
    }
  },
  async post<T>(url: string, data?: unknown, options?: Options): Promise<T> {
    try {
      return await kyInstance.post(await resolveApiUrl(url), { ...options, json: data }).json<T>();
    } catch (error) {
      return handleError(error);
    }
  },
  async postMultipart<T>(url: string, formData: FormData, options?: Options): Promise<T> {
    try {
      return await kyInstance.post(await resolveApiUrl(url), { ...options, body: formData }).json<T>();
    } catch (error) {
      return handleError(error);
    }
  },
  async put<T>(url: string, data?: unknown, options?: Options): Promise<T> {
    try {
      return await kyInstance.put(await resolveApiUrl(url), { ...options, json: data }).json<T>();
    } catch (error) {
      return handleError(error);
    }
  },
  async patch<T>(url: string, data?: unknown, options?: Options): Promise<T> {
    try {
      return await kyInstance.patch(await resolveApiUrl(url), { ...options, json: data }).json<T>();
    } catch (error) {
      return handleError(error);
    }
  },
  async delete<T>(url: string, options?: Options): Promise<T> {
    try {
      return await kyInstance.delete(await resolveApiUrl(url), options).json<T>();
    } catch (error) {
      return handleError(error);
    }
  },
};

export async function requestWebAuthToken(password: string): Promise<WebAuthTokenResponse> {
  const response = await api.post<WebAuthTokenResponse>("auth/token", { password });
  setWebAuthToken(response.token, response.expiresAt);
  return response;
}

export interface SSEEvent<T> {
  event: string;
  data: T;
  id?: string;
}

export interface SSECallbacks<T> {
  onMessage: (event: SSEEvent<T>) => void;
  onError?: (error: Error) => void;
  onOpen?: () => void;
  onClose?: () => void;
}

/**
 * Create an SSE connection using ky (supports auth headers)
 */
async function sse<T>(
  url: string,
  callbacks: SSECallbacks<T>,
  options?: Options & { signal?: AbortSignal },
): Promise<void> {
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;

  try {
    const response = await kyInstance.get(await resolveApiUrl(url), {
      ...options,
      headers: {
        ...options?.headers,
        Accept: "text/event-stream",
      },
      timeout: false,
    });

    callbacks.onOpen?.();

    reader = response.body?.getReader();
    if (!reader) {
      throw new ApiError("Response body is not readable", 0);
    }

    const decoder = new TextDecoder();
    let buffer = "";
    let currentEvent = "message";
    let currentData = "";
    let currentId: string | undefined;

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";

      for (const line of lines) {
        const trimmedLine = line.replace(/\r$/, "");
        if (trimmedLine.startsWith("event:")) {
          currentEvent = trimmedLine.slice(6).trim();
        } else if (trimmedLine.startsWith("data:")) {
          currentData += (currentData ? "\n" : "") + trimmedLine.slice(5).trim();
        } else if (trimmedLine.startsWith("id:")) {
          currentId = trimmedLine.slice(3).trim();
        } else if (trimmedLine === "") {
          if (currentData) {
            try {
              const data = JSON.parse(currentData) as T;
              callbacks.onMessage({ event: currentEvent, data, id: currentId });
            } catch {
              // Ignore JSON parse error
            }
          }
          currentEvent = "message";
          currentData = "";
          currentId = undefined;
        }
      }
    }
  } catch (error) {
    if (error instanceof Error && error.name === "AbortError") {
      // Ignored: intentional abort
    } else {
      try {
        await handleError(error);
      } catch (handledError) {
        callbacks.onError?.(
          handledError instanceof Error ? handledError : new Error(String(handledError)),
        );
      }
    }
  } finally {
    reader?.releaseLock();
    callbacks.onClose?.();
  }
}

export { sse };
export default api;
