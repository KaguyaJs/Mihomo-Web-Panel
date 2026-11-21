import { mount, flushPromises } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import AuthScreen from "./AuthScreen.vue";

describe("authentication screens", () => {
  it("logs in and clears the entered password", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ username: "admin", csrf_token: "csrf" }), {
        status: 200,
      }),
    );
    const wrapper = mount(AuthScreen, { props: { setup: false } });
    await wrapper.get('input[name="username"]').setValue("admin");
    await wrapper
      .get('input[name="password"]')
      .setValue("long enough password");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.emitted("login")?.[0]?.[0]).toEqual({
      username: "admin",
      csrf_token: "csrf",
    });
    expect(
      (wrapper.get('input[name="password"]').element as HTMLInputElement).value,
    ).toBe("");
    wrapper.unmount();
  });
  it("validates setup passwords and requires a successful connection test", async () => {
    const fetcher = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(
        async (path) =>
          new Response(
            JSON.stringify(
              path === "/api/setup/test"
                ? { connected: true, version: "v1.19" }
                : { initialized: true },
            ),
            { status: 200 },
          ),
      );
    const wrapper = mount(AuthScreen, { props: { setup: true } });
    await wrapper.get('input[name="bootstrap-token"]').setValue("a".repeat(43));
    await wrapper.get('input[name="username"]').setValue("admin");
    await wrapper.get('input[name="password"]').setValue("short");
    await wrapper.get('input[name="confirm-password"]').setValue("short");
    await wrapper.get("form").trigger("submit");
    expect(wrapper.text()).toContain("至少 12");
    expect(fetcher).not.toHaveBeenCalled();
    await wrapper
      .get('input[name="password"]')
      .setValue("a long safe password");
    await wrapper
      .get('input[name="confirm-password"]')
      .setValue("a long safe password");
    await wrapper.get("form").trigger("submit");
    expect(wrapper.text()).toContain("连接你的 Mihomo");
    expect(
      wrapper.get('button[type="submit"]').attributes("disabled"),
    ).toBeDefined();
    await wrapper
      .get('input[name="controller"]')
      .setValue("http://127.0.0.1:9090");
    await wrapper.get('input[name="secret"]').setValue("test secret");
    await wrapper
      .findAll("button")
      .find((button) => button.text().includes("测试连接"))!
      .trigger("click");
    await flushPromises();
    expect(
      wrapper.get('button[type="submit"]').attributes("disabled"),
    ).toBeUndefined();
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.emitted("initialized")?.[0]).toEqual(["admin"]);
    expect(wrapper.html()).not.toContain("test secret");
    wrapper.unmount();
  });
  it("shows a failed login without emitting a logged-in state", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          error: "invalid_credentials",
          message: "用户名或密码不正确",
        }),
        { status: 401 },
      ),
    );
    const wrapper = mount(AuthScreen, { props: { setup: false } });
    await wrapper.get('input[name="username"]').setValue("admin");
    await wrapper.get('input[name="password"]').setValue("wrong password");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toBe("用户名或密码不正确");
    expect(wrapper.emitted("login")).toBeUndefined();
    wrapper.unmount();
  });
});
