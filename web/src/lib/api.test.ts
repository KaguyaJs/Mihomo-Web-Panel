import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, setCsrf } from "./api";
afterEach(() => setCsrf(""));
describe("same-origin API", () => {
  it("sends CSRF for mutations and never writes authentication to storage", async () => {
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ saved: true }), { status: 200 }),
      );
    const storage = vi.spyOn(Storage.prototype, "setItem");
    setCsrf("random-csrf");
    await api("/api/config", { method: "PATCH", body: { mode: "rule" } });
    expect(fetcher).toHaveBeenCalledWith(
      "/api/config",
      expect.objectContaining({
        credentials: "same-origin",
        headers: expect.objectContaining({ "X-CSRF-Token": "random-csrf" }),
        body: '{"mode":"rule"}',
      }),
    );
    expect(storage).not.toHaveBeenCalled();
  });
  it("uses friendly backend errors without inventing success", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          error: "backend_timeout",
          message: "Mihomo 响应超时，请稍后重试",
        }),
        { status: 504 },
      ),
    );
    await expect(api("/api/proxies")).rejects.toMatchObject({
      status: 504,
      code: "backend_timeout",
    });
  });
  it("rejects external URLs and sanitizes network errors", async () => {
    await expect(api("https://example.com")).rejects.toThrow(
      "Invalid API path",
    );
    vi.spyOn(globalThis, "fetch").mockRejectedValue(
      new Error("internal socket path /private/secret"),
    );
    try {
      await api("/api/status");
    } catch (cause) {
      expect(cause).toBeInstanceOf(ApiError);
      expect((cause as Error).message).not.toContain("/private/secret");
    }
  });
  it("does not expire a rotated session because an older request failed", async () => {
    let resolve!: (value: Response) => void;
    vi.spyOn(globalThis, "fetch").mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const listener = vi.fn();
    window.addEventListener("session-expired", listener);
    setCsrf("old");
    const pending = api("/api/proxies");
    setCsrf("new");
    resolve(
      new Response(
        JSON.stringify({ error: "unauthenticated", message: "登录已失效" }),
        { status: 401 },
      ),
    );
    await expect(pending).rejects.toThrow();
    expect(listener).not.toHaveBeenCalled();
    window.removeEventListener("session-expired", listener);
  });
});
