// The app's activity orb, without React. It paints the same frames the
// ThinkingOrb component does: the engine's own frame functions on a 2D canvas.
//
//   <canvas data-orb="working" data-size="20"></canvas>
//   window.PoiesisOrbs.mount(root)   start every canvas under root
//   window.PoiesisOrbs.set(canvas, "searching")   change what it shows
import { MODE_FRAMES, paintFrame, resolvePreset } from "thinking-orbs/engine";

const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const live = new Map();
let raf = 0;

function isDark() {
  return document.documentElement.getAttribute("data-mode") === "dark";
}

function setup(canvas) {
  const size = Number(canvas.dataset.size) || 20;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  canvas.width = canvas.height = Math.round(size * dpr);
  canvas.style.width = canvas.style.height = size + "px";
  const { mode, speed, opts } = resolvePreset(canvas.dataset.orb || "working", size);
  return { ctx: canvas.getContext("2d"), size, dpr, frameFn: MODE_FRAMES[mode], speed, opts };
}

function draw(o, t) {
  o.ctx.setTransform(o.dpr, 0, 0, o.dpr, 0, 0);
  o.ctx.clearRect(0, 0, o.size, o.size);
  paintFrame(o.ctx, o.frameFn(o.size, t * o.speed, o.opts), isDark());
}

function loop() {
  raf = 0;
  if (document.visibilityState === "hidden") return;
  const t = performance.now() / 1000;
  live.forEach((o, canvas) => {
    if (!canvas.isConnected) live.delete(canvas);
    else draw(o, t);
  });
  if (live.size) raf = requestAnimationFrame(loop);
}

function start() {
  if (!raf && !reduced && live.size) raf = requestAnimationFrame(loop);
}

function add(canvas) {
  const o = setup(canvas);
  live.set(canvas, o);
  draw(o, reduced ? 0.6 : performance.now() / 1000);
  start();
}

document.addEventListener("visibilitychange", start);

window.PoiesisOrbs = {
  mount(root = document) {
    root.querySelectorAll("canvas[data-orb]").forEach(add);
  },
  set(canvas, state) {
    canvas.dataset.orb = state;
    add(canvas);
  },
};
