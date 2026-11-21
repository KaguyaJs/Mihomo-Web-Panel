import { mount, flushPromises } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import ProxiesPage from "./ProxiesPage.vue";
import type { ProxyNode } from "../types";
const group: ProxyNode = {
  name: "PROXY",
  type: "Selector",
  all: ["节点 A", "节点 B"],
  now: "节点 A",
  history: [],
  udp: true,
  alive: null,
};
const proxies = {
  PROXY: group,
  自动选择: { ...group, name: "自动选择", type: "URLTest" },
  "节点 A": { ...group, name: "节点 A", type: "Vless", all: [] },
  "节点 B": { ...group, name: "节点 B", type: "Trojan", all: [] },
};
describe("proxy selection", () => {
  it("waits for the backend response before updating the selected node", async () => {
    let complete!: (response: Response) => void;
    vi.spyOn(globalThis, "fetch").mockReturnValue(
      new Promise((resolve) => {
        complete = resolve;
      }),
    );
    const wrapper = mount(ProxiesPage, {
      props: { proxies, loading: false, connected: true },
    });
    await wrapper.get('[aria-label="选择 节点 B"]').trigger("click");
    expect(wrapper.emitted("update")).toBeUndefined();
    complete(
      new Response(JSON.stringify({ ...group, now: "节点 B" }), {
        status: 200,
      }),
    );
    await flushPromises();
    expect(wrapper.emitted("update")?.[0]).toEqual([
      "PROXY",
      { ...group, now: "节点 B" },
    ]);
    wrapper.unmount();
  });
  it("does not offer manual selection for URLTest groups or offline backends", () => {
    const wrapper = mount(ProxiesPage, {
      props: { proxies, loading: false, connected: false },
    });
    expect(
      wrapper.get('[aria-label="查看 节点 B"]').attributes("disabled"),
    ).toBeDefined();
    expect(
      wrapper.get('[aria-label="选择 节点 B"]').attributes("disabled"),
    ).toBeDefined();
    wrapper.unmount();
  });
  it("paginates large node groups and searches across all pages", async () => {
    const names = Array.from({ length: 250 }, (_, index) => `节点 ${index}`);
    const wrapper = mount(ProxiesPage, {
      props: {
        proxies: { PROXY: { ...group, all: names } },
        loading: false,
        connected: true,
      },
    });
    expect(wrapper.findAll(".node-row")).toHaveLength(60);
    await wrapper.get('input[type="search"]').setValue("节点 249");
    expect(wrapper.findAll(".node-row")).toHaveLength(1);
    expect(wrapper.text()).toContain("节点 249");
    wrapper.unmount();
  });
});
