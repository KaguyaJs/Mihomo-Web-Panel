import { mount, flushPromises } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import SettingsPage from "./SettingsPage.vue";
const service = {
  version: "0.1.0",
  uptime_seconds: 120,
  platform: "linux",
  architecture: "x86_64",
  active_listen: "127.0.0.1:8787",
  active_origin: "https://panel.example.com",
  secure_cookies: true,
  restart_required: false,
  settings: {
    listen: "127.0.0.1:8787",
    public_origin: "https://panel.example.com",
    session_idle_seconds: 1800,
    session_absolute_seconds: 86400,
    traffic_interval_ms: 1000,
  },
};
function fetchMock() {
  return vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(async (path, options) => {
      const url = String(path);
      let body: unknown = {};
      if (url === "/api/settings" && !options?.body) body = service;
      else if (url === "/api/settings/sessions") body = { sessions: [] };
      else if (url === "/api/settings/autostart")
        body = {
          supported: false,
          enabled: false,
          active: "unsupported",
          message: "未安装权限辅助程序",
        };
      else if (url === "/api/settings/security")
        body = { username: "admin", csrf_token: "rotated" };
      else if (url.includes("/backend"))
        body = { connected: true, version: "v1.19" };
      else body = { saved: true, restart_required: false };
      return new Response(JSON.stringify(body), { status: 200 });
    });
}
describe("settings center", () => {
  it("changes the password with confirmation and updates the current authentication state", async () => {
    const fetcher = fetchMock();
    const wrapper = mount(SettingsPage, {
      props: {
        user: { username: "admin", csrf_token: "csrf" },
        status: { configured: true, connected: true, version: "v1.19" },
      },
    });
    await flushPromises();
    const fields = wrapper.findAll('input[type="password"]');
    await fields[0]!.setValue("old password 2026");
    await fields[1]!.setValue("new password 2026");
    await fields[2]!.setValue("new password 2026");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(fetcher).toHaveBeenCalledWith(
      "/api/settings/security",
      expect.objectContaining({
        method: "PATCH",
        body: expect.stringContaining("new password 2026"),
      }),
    );
    expect(wrapper.emitted("user")?.[0]?.[0]).toEqual({
      username: "admin",
      csrf_token: "rotated",
    });
    expect((fields[0]!.element as HTMLInputElement).value).toBe("");
    wrapper.unmount();
  });
  it("saves backend settings only after a connection test and never receives saved credentials", async () => {
    const fetcher = fetchMock();
    const wrapper = mount(SettingsPage, {
      props: {
        user: { username: "admin", csrf_token: "csrf" },
        status: { configured: true, connected: true, version: "v1.19" },
      },
    });
    await flushPromises();
    await wrapper.findAll(".settings-tabs button")[1]!.trigger("click");
    const button = () =>
      wrapper.findAll("button").find((item) => item.text() === "保存连接配置")!;
    expect(button().attributes("disabled")).toBeDefined();
    await wrapper.get("input:not([type])").setValue("http://127.0.0.1:9090");
    await wrapper.get('input[type="password"]').setValue("new-secret");
    await wrapper
      .findAll("button")
      .find((item) => item.text() === "测试连接")!
      .trigger("click");
    await flushPromises();
    expect(button().attributes("disabled")).toBeUndefined();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(fetcher).toHaveBeenCalledWith(
      "/api/settings/backend",
      expect.objectContaining({
        method: "PUT",
        body: expect.stringContaining("new-secret"),
      }),
    );
    expect(
      (wrapper.get('input[type="password"]').element as HTMLInputElement).value,
    ).toBe("");
    wrapper.unmount();
  });
});
