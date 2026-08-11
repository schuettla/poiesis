import { describe, expect, it } from "vitest";
import { detectIntent } from "./mediaIntent";

describe("detectIntent", () => {
  it("reads a leading imperative as image intent", () => {
    expect(detectIntent("draw a fox reading a map in a pine forest")).toEqual({
      intent: "image",
      confidence: "high",
    });
  });

  it("does not fire on a question that merely mentions drawing", () => {
    expect(detectIntent("how do I draw a fox in Illustrator?")).toEqual({
      intent: "chat",
      confidence: "low",
    });
  });

  it("reads a leading imperative naming video as video intent", () => {
    expect(detectIntent("make a video of a fox running")).toEqual({
      intent: "video",
      confidence: "high",
    });
  });

  it("reads an attached image plus imperative edit language as edit intent", () => {
    expect(
      detectIntent("remove the background", [
        { id: "a1", kind: "image", name: "photo.jpg", path: "/tmp/photo.jpg" },
      ])
    ).toEqual({ intent: "edit", confidence: "high" });
  });

  it("reads an attached image plus a question as chat intent (vision Q&A)", () => {
    expect(
      detectIntent("what is in this photo?", [
        { id: "a1", kind: "image", name: "photo.jpg", path: "/tmp/photo.jpg" },
      ])
    ).toEqual({ intent: "chat", confidence: "high" });
  });

  it("defaults to low-confidence chat with nothing to go on", () => {
    expect(detectIntent("what's the weather like")).toEqual({ intent: "chat", confidence: "low" });
  });
});
