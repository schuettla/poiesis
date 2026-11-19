import { describe, expect, it } from "vitest";
import type { EngineInfo, Persona, Team } from "./api";
import { deleteWords, placeNote, placesOf, teamNotes } from "./teamNotes";
import type { Model } from "./types";

const persona = (id: string, name: string, extra: Partial<Persona> = {}): Persona =>
  ({ id, name, system_prompt: "x", model_id: null, params_json: null, is_default: false, created_at: 0, updated_at: 0, tools_json: null, skills_json: null, description: null, spawnable: false, ...extra }) as Persona;
const model = (id: string, tier: Model["tier"]): Model => ({ id, name: id, provenance: "local", tier }) as Model;

const personas = [
  persona("mira", "Mira", { model_id: "small" }),
  persona("kai", "Kai", { model_id: "chatty" }),
  persona("juno", "Juno", { model_id: "big" }),
  persona("app", "Cody", { spawnable: true, engine_id: "claude-code" }),
];
const models = [model("small", "basic"), model("chatty", "chat"), model("big", "full")];
const engines = [{ id: "claude-code", name: "Claude Code", memory_ready: true, memory_tools: false }] as unknown as EngineInfo[];

describe("tier notes", () => {
  it("a coordinator needs a Full model", () => {
    expect(placeNote("coordinator", "persona:mira", personas, models, engines)?.text).toBe("Mira works on a Basic model. A coordinator needs a Full model to plan.");
    expect(placeNote("coordinator", "persona:juno", personas, models, engines)).toBeNull();
  });

  it("a member on a Chat only model fits only as a reviewer or an adviser, and Basic is fine", () => {
    expect(placeNote("ui", "persona:kai", personas, models, engines)?.text).toContain("fits best as a reviewer or an adviser");
    expect(placeNote("ui", "persona:mira", personas, models, engines)).toBeNull();
  });

  it("a persona on an agent app says it posts only with my tools on", () => {
    const text = placeNote("ui", "persona:app", personas, models, engines)?.text ?? "";
    expect(text).toContain("Cody runs on Claude Code");
    expect(text).toContain("only when my tools are switched on");
  });

  it("a helper, Poiesis and a persona without a model of its own have no note", () => {
    expect(placeNote("ui", "helper", personas, models, engines)).toBeNull();
    expect(placeNote("coordinator", "poiesis", personas, models, engines)).toBeNull();
    expect(placeNote("ui", "persona:ghost", personas, models, engines)).toBeNull();
  });

  it("a team has the coordinator's note and each member's", () => {
    const team = {
      coordinator: "persona:mira",
      members: [
        { role: "a", persona: "persona:kai", job: "", owns: [] },
        { role: "b", persona: "helper", job: "", owns: [] },
      ],
    } as unknown as Team;
    expect(teamNotes(team, personas, models, engines).map((n) => n.who)).toEqual(["coordinator", "a"]);
  });
});

describe("where a persona sits, and what deleting it does", () => {
  const teams = [
    { slug: "shop", name: "Shop team", coordinator: "poiesis", members: [{ role: "verifier", persona: "persona:p1", job: "", owns: [] }] },
    { slug: "desk", name: "Research desk", coordinator: "persona:p1", members: [] },
    { slug: "other", name: "Other", coordinator: "poiesis", members: [{ role: "x", persona: "persona:p2", job: "", owns: [] }] },
  ] as unknown as Team[];

  it("lists the member places and the coordinator places", () => {
    expect(placesOf("p1", teams)).toEqual([
      { slug: "shop", team: "Shop team", role: "verifier" },
      { slug: "desk", team: "Research desk", role: "coordinator" },
    ]);
    expect(placesOf("nobody", teams)).toEqual([]);
  });

  it("says it as the plan does", () => {
    expect(deleteWords("Mira", 12, [{ slug: "shop", team: "Shop team", role: "verifier" }])).toBe(
      "Mira leads 12 chats and is in Shop team. The chats go back to Poiesis; in Shop team, Mira's place becomes a helper with the same job."
    );
    expect(deleteWords("Mira", 0, [])).toBe("Mira leads no chats.");
    expect(deleteWords("Mira", 1, [])).toBe("Mira leads 1 chat. The chat goes back to Poiesis.");
    expect(deleteWords("Mira", 0, [{ slug: "desk", team: "Research desk", role: "coordinator" }])).toBe(
      "Mira leads no chats and is in Research desk. Poiesis coordinates Research desk."
    );
  });
});
