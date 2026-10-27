/**
 * @vitest-environment jsdom
 *
 * `USE-UI-9`: the browser stopped at a login, a captcha or a page the quick check
 * could not place. The card shows the page, says what was seen and how sure, and
 * offers to hand the browser over, skip the step, or say the stop was wrong.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PermissionPanel from "./PermissionPanel";
import { useAppStore } from "../../lib/store";
import type { PermissionRequest } from "../../lib/api";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  inTauri: () => true,
  readImageDataUri: vi.fn(async () => "data:image/png;base64,AAAA"),
}));

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const resolvePermission = vi.fn();

beforeEach(() => {
  resolvePermission.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  useAppStore.setState({ pendingPermissions: [] } as never);
});

const card = (over: Partial<NonNullable<PermissionRequest["page"]>> = {}, summary = "This page wants a login. Take over?"): PermissionRequest => ({
  id: "perm_p1",
  summary,
  path: "accounts.shopline.example",
  mode: "read",
  in_folder: false,
  capability: "page",
  page: {
    host: "accounts.shopline.example",
    wall: "login",
    screenshot: "C:\\data\\browser-screenshots\\a.png",
    note: "login page \u00b7 quick decision (0.91)",
    ...over,
  },
});

async function show(r: PermissionRequest) {
  useAppStore.setState({ pendingPermissions: [r], resolvePermission } as never);
  await act(async () => root.render(<PermissionPanel />));
}

const buttons = () => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).map((b) => b.textContent);
const press = (label: string) =>
  act(() => Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) => b.textContent === label)!.click());

describe("the page card", () => {
  it("names the site, asks the question and says what was seen and how sure", async () => {
    await show(card());
    expect(container.textContent).toContain("The browser stopped on accounts.shopline.example");
    expect(container.querySelector(".permission-summary")?.textContent).toBe("This page wants a login. Take over?");
    expect(container.textContent).toContain("login page \u00b7 quick decision (0.91)");
  });

  it("shows the screenshot the quick check looked at", async () => {
    await show(card());
    const img = container.querySelector<HTMLImageElement>(".permission-shot img");
    expect(img?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
    expect(img?.getAttribute("alt")).toBe("The page at accounts.shopline.example");
  });

  it("still asks when the picture is gone", async () => {
    await show(card({ screenshot: undefined }));
    expect(container.querySelector(".permission-shot")).toBeNull();
    expect(buttons()).toContain("Open browser");
  });

  it("offers open browser and skip, and the link for a wrong stop comes after them", async () => {
    await show(card());
    expect(buttons()).toEqual(["Open browser", "Skip this step", "This page is fine, carry on"]);
  });

  it("sends once for open browser, deny for skip and chat for a page that is fine", async () => {
    await show(card());
    press("Open browser");
    press("Skip this step");
    press("This page is fine, carry on");
    expect(resolvePermission.mock.calls).toEqual([
      ["perm_p1", "once"],
      ["perm_p1", "deny"],
      ["perm_p1", "chat"],
    ]);
  });

  it("says what opening the browser does, so nobody is surprised by a window", async () => {
    await show(card({ wall: "captcha" }, "This page wants to know you are a person. Take over?"));
    expect(container.querySelector(".permission-summary")?.textContent).toContain("know you are a person");
    expect(container.textContent).toContain("A window opens on this page.");
    expect(container.textContent).toContain("close it and I carry on");
  });

  it("is not the handover or the ordinary permission card", async () => {
    await show(card());
    expect(container.textContent).not.toContain("Hand it over");
    expect(container.textContent).not.toContain("Always allow");
  });
});
