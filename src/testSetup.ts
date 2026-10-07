// jsdom has no canvas, and logs "Not implemented" every time something asks for
// a 2D context. The activity orbs draw on canvas and tolerate a null context
// (they simply paint nothing), so tests answer null instead of printing noise.
if (typeof HTMLCanvasElement !== "undefined") {
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
}
