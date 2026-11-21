let csrf = "";
export function setCsrf(value: string) {
  csrf = value;
}
export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

export async function api<T>(
  path: string,
  options: { method?: string; body?: unknown; signal?: AbortSignal } = {},
): Promise<T> {
  if (!path.startsWith("/api/")) throw new Error("Invalid API path");
  const method = options.method ?? "GET";
  const requestCsrf = csrf;
  const headers: Record<string, string> = { Accept: "application/json" };
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  if (method !== "GET" && method !== "HEAD" && csrf)
    headers["X-CSRF-Token"] = csrf;
  let response: Response;
  try {
    response = await fetch(path, {
      method,
      headers,
      credentials: "same-origin",
      body:
        options.body === undefined ? undefined : JSON.stringify(options.body),
      signal: options.signal,
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError")
      throw error;
    throw new ApiError(
      0,
      "network_error",
      "无法连接管理服务，请检查网络后重试",
    );
  }
  if (!response.ok) {
    const error = await response
      .json()
      .catch(() => ({
        error: "request_failed",
        message:
          response.status === 413 ? "提交内容过大" : "请求未完成，请稍后重试",
      }));
    if (
      response.status === 401 &&
      error.error === "unauthenticated" &&
      requestCsrf === csrf
    )
      window.dispatchEvent(new Event("session-expired"));
    throw new ApiError(response.status, error.error, error.message);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}
