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

  it("does not fire on a bare 'create' request with no visual noun", () => {
    expect(detectIntent("create a React component for the settings page")).toEqual({
      intent: "chat",
      confidence: "low",
    });
    expect(detectIntent("make a plan for the migration")).toEqual({
      intent: "chat",
      confidence: "low",
    });
    expect(detectIntent("generate a changelog from these commits")).toEqual({
      intent: "chat",
      confidence: "low",
    });
  });

  it("fires when a generic verb names something visual", () => {
    expect(detectIntent("create an image of a fox reading a map")).toEqual({
      intent: "image",
      confidence: "high",
    });
    expect(detectIntent("generate a logo for my coffee shop")).toEqual({
      intent: "image",
      confidence: "high",
    });
  });

  it("still fires on a visual verb with no object noun", () => {
    expect(detectIntent("paint something moody and blue")).toEqual({
      intent: "image",
      confidence: "high",
    });
  });
});
