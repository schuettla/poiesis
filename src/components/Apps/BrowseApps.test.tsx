/**
 * @vitest-environment jsdom
 *
 * `CA-UI-2`: Browse apps lists the whole bundled catalog with a count, keeps one
 * size whatever is chosen, draws a mark or a letter, and connects on a click.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Catalog, CatalogApp, ComposioStatus } from "../../lib/api";

const composioConnect = vi.fn();
const composioCatalog = vi.fn();
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  composioConnect: (...a: unknown[]) => composioConnect(...a),
  composioCatalog: (...a: unknown[]) => composioCatalog(...a),
}));

import BrowseApps from "./BrowseApps";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const app = (i: number, category = "Email"): CatalogApp => ({
  slug: `app${i}`,
  name: `App ${i}`,
  description: `Does thing ${i}.`,
  categories: [category],
  managed: true,
  logo: i % 2 === 0 ? `/app-logos/app${i}.svg` : undefined,
});

const catalogOf = (apps: CatalogApp[], total = apps.length): Catalog => ({
  apps,
  partial: false,
  source: "bundled",
  note: null,
  total,
});

const status = (over: Partial<ComposioStatus> = {}): ComposioStatus => ({
  configured: true,
  mode: "connect",
  connector_id: "conn",
  enabled: true,
  tool_count: 4,
  checked_at: 1,
  last_error: null,
  accounts: [],
  policies: [],
  ...over,
});

// The search waits a tick before it asks, so a test waits one too.
const settle = () => act(async () => void (await new Promise((r) => setTimeout(r, 10))));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  composioConnect.mockReset();
  composioCatalog.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

async function open(catalog: Catalog, s: ComposioStatus | null = status()) {
  composioCatalog.mockResolvedValue(catalog);
  await act(async () => {
    root.render(<BrowseApps status={s} reload={vi.fn()} onClose={vi.fn()} />);
  });
  await settle();
}

describe("Browse apps", () => {
  it("says how many apps there are", async () => {
    await open(catalogOf(Array.from({ length: 1608 }, (_, i) => app(i))));
    expect(container.querySelector(".browse-count")?.textContent).toBe("1,608 apps");
  });

  it("draws the first sixty and adds sixty more on request", async () => {
    await open(catalogOf(Array.from({ length: 130 }, (_, i) => app(i))));
    expect(container.querySelectorAll(".browse-app").length).toBe(60);
    const more = [...container.querySelectorAll("button")].find((b) => b.textContent === "Show more")!;
    await act(async () => more.click());
    expect(container.querySelectorAll(".browse-app").length).toBe(120);
  });

  it("shows a mark where there is one and a letter where there is not", async () => {
    await open(catalogOf([app(2), app(3)]));
    const tiles = container.querySelectorAll(".app-logo");
    expect(tiles[0].querySelector("img")?.getAttribute("src")).toBe("/app-logos/app2.svg");
    expect(tiles[1].querySelector("img")).toBeNull();
    expect(tiles[1].textContent).toBe("A");
  });

  it("falls back to the letter when a mark does not load", async () => {
    await open(catalogOf([app(2)]));
    const img = container.querySelector(".app-logo img")!;
    await act(async () => void img.dispatchEvent(new Event("error")));
    expect(container.querySelector(".app-logo img")).toBeNull();
    expect(container.querySelector(".app-logo")?.textContent).toBe("A");
  });

  it("keeps the same dialog whatever the list holds", async () => {
    await open(catalogOf([]));
    expect(container.querySelectorAll(".browse-card").length).toBe(1);
    expect(container.querySelector(".browse-grid")).not.toBeNull();
    expect(container.querySelector(".browse-empty")?.textContent).toContain("couldn't find");
  });

  it("connects an app on a click", async () => {
    composioConnect.mockResolvedValue({ id: "connect:app1", status: "INITIATED" });
    await open(catalogOf([app(1)]));
    const connect = [...container.querySelectorAll("button")].find((b) => b.textContent === "Connect")!;
    await act(async () => connect.click());
    expect(composioConnect).toHaveBeenCalledWith("app1", undefined);
  });

  it("lets a person look without a key, but not connect", async () => {
    await open(catalogOf([app(1)]), status({ configured: false, mode: null }));
    expect(container.querySelector(".browse-key-note")?.textContent).toContain("look at every app");
    const connect = [...container.querySelectorAll("button")].find((b) => b.textContent === "Connect")!;
    expect(connect.hasAttribute("disabled")).toBe(true);
  });

  it("says why a waiting sign-in could not be checked", async () => {
    composioCatalog.mockResolvedValue(catalogOf([app(1)]));
    await act(async () => {
      root.render(
        <BrowseApps
          status={status({ accounts: [{ id: "connect:app1", app: "app1", app_name: "App 1", label: null, status: "INITIATED" }] })}
          reload={vi.fn()}
          onClose={vi.fn()}
          checkError="Composio didn't answer."
        />,
      );
    });
    await settle();
    expect(container.textContent).toContain("I couldn't check: Composio didn't answer.");
  });

  /** `CAP-C6`: with a Connect key a working app takes one more account, by name. */
  it("adds one more account by name with a Connect key, and a first one without", async () => {
    composioConnect.mockResolvedValue({ id: "connect:app1:new", app: "app1", app_name: "App 1", label: "work", status: "INITIATED" });
    composioCatalog.mockResolvedValue(catalogOf([app(1), app(2)]));
    await act(async () => {
      root.render(
        <BrowseApps
          status={status({ accounts: [{ id: "connect:app1", app: "app1", app_name: "App 1", label: null, status: "ACTIVE" }] })}
          reload={vi.fn()}
          onClose={vi.fn()}
        />,
      );
    });
    await settle();
    expect(container.textContent).toContain("Connected");
    const add = [...container.querySelectorAll("button")].find((b) => b.textContent === "Add account")!;
    act(() => add.click());
    const field = container.querySelector<HTMLInputElement>("input[aria-label='Name for the new App 1 account']")!;
    act(() => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(field, "work");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const go = [...container.querySelectorAll(".browse-app-label button")].find((b) => b.textContent === "Connect")!;
    await act(async () => (go as HTMLButtonElement).click());
    expect(composioConnect).toHaveBeenCalledWith("app1", "work");

    // An app with no account yet is connected with no name asked for.
    const first = [...container.querySelectorAll("button")].find((b) => b.textContent === "Connect")!;
    await act(async () => first.click());
    expect(composioConnect).toHaveBeenLastCalledWith("app2", undefined);
  });
});
