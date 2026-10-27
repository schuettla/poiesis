import { describe, expect, it } from "vitest";
import type { SearchHit } from "./api";
import { buildContextRefs } from "./store";

/** `CRD-1`: the chat window records why each recalled entry came, as the backend does. */
describe("the record kept beside an answer", () => {
  const hit = (title: string, kind: string, why?: string): SearchHit => ({
    source: "memory",
    conversation_id: null,
    title,
    created_at: 0,
    snippet: "",
    kind,
    why,
  });

  it("keeps the reason for every memory that came by relevance", () => {
    const refs = buildContextRefs({
      personaId: null,
      memory: undefined,
      injectedFacts: ["be-concise", "anna-contact"],
      matches: [hit("anna-contact", "preference", "you named Anna"), hit("check-header", "lesson", "about check header")],
    });
    expect(refs.facts).toEqual(["be-concise", "anna-contact"]);
    expect(refs.lessons).toEqual(["check-header"]);
    expect(refs.why).toEqual({ "anna-contact": "you named Anna", "check-header": "about check header" });
  });

  it("has no reasons when recall gave none", () => {
    const refs = buildContextRefs({ personaId: null, memory: undefined, injectedFacts: [], matches: [hit("a", "fact")] });
    expect(refs.why).toEqual({});
  });
});
