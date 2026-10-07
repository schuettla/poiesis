/* One scripted run in the Poiesis window: type a question, send it, watch the
   agent work, read the answer, see what it saved. Everything it shows is the
   app's own markup and CSS (see index.html); this file only adds and removes
   it in the order the real app does. */
(function () {
  "use strict";

  var doc = document;
  var $ = function (s, el) { return (el || doc).querySelector(s); };
  var $$ = function (s, el) { return Array.prototype.slice.call((el || doc).querySelectorAll(s)); };
  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------- the app's icon set (src/components/Icons/Icons.tsx) ---------- */
  var ICONS = {
    plus: ['<path d="M10 4.5v11M4.5 10h11" stroke-linecap="round"/>', 1.3],
    search: ['<circle cx="8.8" cy="8.8" r="5"/><path d="M12.5 12.5 16.5 16.5" stroke-linecap="round"/>', 1.3],
    bookmark: ['<path d="M5.5 3.5h9a1 1 0 0 1 1 1V17l-5.5-3.2L4.5 17V4.5a1 1 0 0 1 1-1z" stroke-linejoin="round"/>', 1.3],
    folder: ['<path d="M2.5 5.5A1.5 1.5 0 0 1 4 4h3.2l1.4 1.8H16a1.5 1.5 0 0 1 1.5 1.5v7.2A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z" stroke-linejoin="round"/>', 1.3],
    settings: ['<circle cx="10" cy="10" r="2.6"/><path d="M10 2.8v2.3M10 14.9v2.3M17.2 10h-2.3M5.1 10H2.8M15.1 4.9l-1.6 1.6M6.5 13.5l-1.6 1.6M15.1 15.1l-1.6-1.6M6.5 6.5 4.9 4.9" stroke-linecap="round"/>', 1.3],
    kebab: ['<circle cx="4.5" cy="10" r="1.4" fill="currentColor" stroke="none"/><circle cx="10" cy="10" r="1.4" fill="currentColor" stroke="none"/><circle cx="15.5" cy="10" r="1.4" fill="currentColor" stroke="none"/>', 1.3],
    chevR: ['<path d="M8 4.5 13.5 10 8 15.5" stroke-linecap="round" stroke-linejoin="round"/>', 1.5],
    chevU: ['<path d="M4.5 12.5 10 7l5.5 5.5" stroke-linecap="round" stroke-linejoin="round"/>', 1.5],
    sidebarL: ['<rect x="2.5" y="3.5" width="15" height="13" rx="2.5"/><line x1="7.7" y1="3.5" x2="7.7" y2="16.5"/>', 1.3],
    sidebarR: ['<rect x="2.5" y="3.5" width="15" height="13" rx="2.5"/><line x1="12.3" y1="3.5" x2="12.3" y2="16.5"/>', 1.3],
    refresh: ['<path d="M16 10a6 6 0 1 1-1.8-4.3M16 3v3h-3" stroke-linecap="round" stroke-linejoin="round"/>', 1.3],
    treeFolder: ['<path d="M2.5 6A1.5 1.5 0 0 1 4 4.5h3.2l1.4 1.8H16A1.5 1.5 0 0 1 17.5 8v6A1.5 1.5 0 0 1 16 15.5H4A1.5 1.5 0 0 1 2.5 14z" stroke-linejoin="round"/>', 1.2],
    treeFolderOpen: ['<path d="M2.5 15V6A1.5 1.5 0 0 1 4 4.5h3.2l1.4 1.8H15A1.5 1.5 0 0 1 16.5 8H5.6L2.5 15z" stroke-linejoin="round"/>', 1.2],
    file: ['<path d="M5 3.5h6L15 7.5v9H5z" stroke-linejoin="round"/><path d="M11 3.5v4h4" stroke-linejoin="round"/>', 1.2],
  };
  $$("i[data-ic]").forEach(function (el) {
    var def = ICONS[el.dataset.ic];
    if (!def) return;
    var size = el.dataset.size || 14;
    var sw = el.dataset.sw || def[1];
    var cls = el.dataset.cls ? ' class="' + el.dataset.cls + '"' : "";
    el.outerHTML =
      '<svg' + cls + ' width="' + size + '" height="' + size + '" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="' + sw + '" aria-hidden="true">' +
      def[0] + "</svg>";
  });

  // A picture of the window, not a form: nothing in it takes the keyboard.
  $$("button, li[tabindex]").forEach(function (el) { el.setAttribute("tabindex", "-1"); });

  window.PoiesisOrbs.mount(doc);

  /* ---------- the window ---------- */
  var el = {
    empty: $("#empty"), stream: $("#stream"), turnUser: $("#turnUser"), run: $("#run"),
    tl: $("#tl"), tlHead: $("#tlHead"), tlTitle: $("#tlTitle"), tlSum: $("#tlSum"), tlRows: $("#tlRows"),
    answer: $("#answer"), thinking: $("#thinking"), meter: $("#meter"), meterText: $("#meterText"),
    meterOrb: $("#meterOrb"), actions: $("#actions"), input: $("#input"), send: $("#send"),
    toast: $("#toast"), top: $("#topLabel"), mark: $("#brandMark"), sessions: $("#projSessions"), count: $("#projCount"),
  };
  var steps = $$(".step", el.tlRows);
  var QUESTION = "Find the Brandt invoice and tell me what I owe.";
  var SUMMARY = "recalled memory 1 · searched 1 · read 1 · 1 more";
  var answerTexts = [];
  var walker = doc.createTreeWalker(el.answer, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) answerTexts.push({ node: walker.currentNode, full: walker.currentNode.nodeValue });
  var paragraphs = $$("p", el.answer);

  function show(node, on) { node.classList.toggle("pre", !on); }
  function setMark(state) {
    el.mark.setAttribute("class", "poiesis-mark stage-2 state-" + state);
    el.mark.setAttribute("aria-label", "Poiesis Agent: " + (state === "active" ? "working" : "resting"));
  }
  function setBusy(busy) {
    el.send.textContent = busy ? "■" : "↑";
    el.send.setAttribute("aria-label", busy ? "Stop generating" : "Send message");
  }
  function clock(s) { return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); }

  var newRow = null;
  function addRailRow() {
    if (newRow) return;
    newRow = doc.createElement("li");
    newRow.className = "chat-row active";
    newRow.innerHTML =
      '<span class="chat-title">Brandt invoice</span><span class="row-slot"><time class="row-stamp">now</time>' +
      '<div class="chat-menu-wrap"><button class="chat-more" aria-label="More actions" tabindex="-1">' + doc.querySelector(".chat-more").innerHTML + "</button></div></span>";
    el.sessions.insertBefore(newRow, el.sessions.firstChild);
    el.count.textContent = "2";
    el.top.textContent = "Brandt invoice";
  }

  function reset() {
    $$(".turn-user, .agent-run, .timeline, .step, .run-text, .thinking, .run-meter-row, .turn-actions, .memory-toast", doc)
      .forEach(function (n) { n.classList.add("pre"); });
    show(el.empty, true);
    el.stream.classList.add("pre");
    el.tl.className = "timeline live open pre";
    el.tlHead.setAttribute("aria-expanded", "true");
    el.tlRows.style.display = "";
    el.tlTitle.textContent = "Working";
    el.tlSum.textContent = "1 step";
    steps.forEach(function (s) { s.classList.remove("running", "error"); $(".result", s).textContent = ""; });
    el.answer.className = "run-text streaming pre";
    answerTexts.forEach(function (t) { t.node.nodeValue = ""; });
    paragraphs.forEach(function (p, i) { p.style.display = i ? "none" : ""; });
    el.toast.classList.remove("arrive");
    el.input.value = "";
    setBusy(false);
    setMark("idle");
    if (newRow) { newRow.remove(); newRow = null; }
    el.count.textContent = "1";
    el.top.textContent = "New chat";
  }

  /* The finished state: what the window rests on. */
  function settle() {
    show(el.empty, false);
    el.stream.classList.remove("pre");
    [el.turnUser, el.run, el.tl, el.answer, el.actions].forEach(function (n) { show(n, true); });
    steps.forEach(function (s) {
      show(s, true);
      s.classList.remove("running");
      $(".result", s).textContent = s.dataset.result;
    });
    el.tl.className = "timeline settled";
    el.tlHead.setAttribute("aria-expanded", "false");
    el.tlRows.style.display = "none";
    el.tlTitle.textContent = "4 steps";
    el.tlSum.textContent = SUMMARY;
    answerTexts.forEach(function (t) { t.node.nodeValue = t.full; });
    paragraphs.forEach(function (p) { p.style.display = ""; });
    el.answer.className = "run-text";
    show(el.thinking, false);
    show(el.meter, false);
    setBusy(false);
    setMark("idle");
    addRailRow();
    show(el.toast, true);
  }

  el.tlHead.addEventListener("click", function () {
    var open = !el.tl.classList.contains("open");
    el.tl.classList.toggle("open", open);
    el.tlRows.style.display = open ? "" : "none";
    el.tlHead.setAttribute("aria-expanded", String(open));
  });
  el.toast.addEventListener("click", function (e) {
    if (e.target.closest("#undo")) show(el.toast, false);
  });

  /* ---------- the scripted run ---------- */
  var token = 0;
  var CANCEL = {};
  var runStart = 0;
  var tick = 0;

  function sleep(ms, mine) {
    return new Promise(function (resolve, reject) {
      setTimeout(function () { (mine === token ? resolve : reject)(CANCEL); }, ms);
    });
  }

  function typeInto(setter, text, perChar, mine) {
    var i = 0;
    return new Promise(function (resolve, reject) {
      (function next() {
        if (mine !== token) return reject(CANCEL);
        i += 1;
        setter(text.slice(0, i));
        if (i < text.length) setTimeout(next, perChar + (text[i - 1] === " " ? 14 : 0));
        else resolve();
      })();
    });
  }

  function streamAnswer(perChar, mine) {
    var chain = Promise.resolve();
    answerTexts.forEach(function (t) {
      chain = chain.then(function () {
        t.node.parentNode.closest("p").style.display = "";
        return typeInto(function (s) { t.node.nodeValue = s; }, t.full, perChar, mine);
      });
    });
    return chain;
  }

  function meterFor(step) {
    var secs = Math.floor((Date.now() - runStart) / 1000);
    el.meterText.textContent = step + " · " + clock(secs) + " · about 2k in context";
  }

  async function run() {
    var mine = ++token;
    clearInterval(tick);
    reset();
    try {
      await sleep(600, mine);
      await typeInto(function (s) { el.input.value = s; }, QUESTION, 28, mine);
      await sleep(400, mine);

      // Send.
      el.input.value = "";
      setBusy(true);
      setMark("active");
      show(el.empty, false);
      el.stream.classList.remove("pre");
      show(el.turnUser, true);
      await sleep(500, mine);
      show(el.run, true);
      show(el.thinking, true);
      runStart = Date.now();
      await sleep(900, mine);

      // Steps.
      show(el.thinking, false);
      show(el.tl, true);
      show(el.meter, true);
      tick = setInterval(function () { if (mine === token) meterFor(el.meterText.dataset.step || "waiting for the model"); }, 1000);
      for (var i = 0; i < steps.length; i++) {
        var s = steps[i];
        show(s, true);
        s.classList.add("running");
        el.tlSum.textContent = i + 1 + (i === 0 ? " step" : " steps");
        el.meterText.dataset.step = s.dataset.verb + " " + s.dataset.target;
        meterFor(el.meterText.dataset.step);
        window.PoiesisOrbs.set(el.meterOrb, s.dataset.orb);
        if (i === 2) addRailRow();
        await sleep(1150, mine);
        s.classList.remove("running");
        $(".result", s).textContent = s.dataset.result;
        await sleep(160, mine);
      }

      // The answer, as it arrives.
      el.meterText.dataset.step = "waiting for the model";
      meterFor("waiting for the model");
      window.PoiesisOrbs.set(el.meterOrb, "composing");
      await sleep(600, mine);
      answerTexts.forEach(function (t) { t.node.nodeValue = ""; });
      show(el.answer, true);
      await streamAnswer(15, mine);
      await sleep(350, mine);

      // Done: the steps fold to one line, the toast says what it kept.
      clearInterval(tick);
      settle();
      el.toast.classList.add("arrive");
    } catch (e) {
      if (e !== CANCEL) throw e;
    }
  }

  function play() {
    if (reduced) { token += 1; clearInterval(tick); reset(); settle(); return; }
    run();
  }

  /* ---------- the page around it ---------- */
  function setMode(mode) { doc.documentElement.setAttribute("data-mode", mode === "dark" ? "dark" : "light"); }
  window.addEventListener("message", function (e) {
    var d = e.data;
    if (!d || typeof d !== "object") return;
    if (d.type === "mode") setMode(d.mode);
    else if (d.type === "play" || d.type === "replay") play();
  });
  if (window.matchMedia("(prefers-color-scheme: dark)").matches) setMode("dark");

  // Opened on its own, it plays itself; inside the site, the page says when.
  if (window.parent === window) play();
  else {
    reset();
    if (reduced) settle();
    window.parent.postMessage({ type: "ready" }, "*");
  }
})();
