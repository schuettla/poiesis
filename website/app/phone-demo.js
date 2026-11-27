/* Shows the screen the address asks for, in the lighting the page around it has. */
(function () {
  "use strict";

  var doc = document;
  var id = location.hash.slice(1);
  // The ids carry a prefix so the address matches no element: a matching one
  // would make the browser scroll the page around this frame to it.
  (doc.getElementById("screen-" + id) || doc.querySelector(".shot-screen")).classList.add("on");

  // A picture of the phone, not a form: nothing in it takes the keyboard.
  Array.prototype.forEach.call(doc.querySelectorAll("button, input, textarea"), function (el) {
    el.setAttribute("tabindex", "-1");
  });

  function setMode(mode) { doc.documentElement.setAttribute("data-mode", mode === "dark" ? "dark" : "light"); }
  window.addEventListener("message", function (e) {
    var d = e.data;
    if (d && typeof d === "object" && d.type === "mode") setMode(d.mode);
  });
  if (window.matchMedia("(prefers-color-scheme: dark)").matches) setMode("dark");
  if (window.parent !== window) window.parent.postMessage({ type: "phone-ready" }, "*");
})();
