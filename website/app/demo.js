/* One scripted run in the Poiesis window: type a question, send it, watch the
   agent work, read the answer, see what it saved. Everything it shows is the
   app's own markup and CSS (see index.html); this file only shows and hides it
   in the order the real app does. The markup rests in the finished state. */
(function () {
  "use strict";

  var doc = document;
  var $ = function (s, el) { return (el || doc).querySelector(s); };
  var $$ = function (s, el) { return Array.prototype.slice.call((el || doc).querySelectorAll(s)); };
  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // A picture of the window, not a form: nothing in it takes the keyboard.
  $$("button, input, [tabindex]").forEach(function (el) { el.setAttribute("tabindex", "-1"); });

  window.PoiesisOrbs.mount(doc);

  /* ---------- the window ---------- */
  var el = {};
  ["empty", "stream", "turnUser", "run", "tlLive", "tlDone", "liveOrb", "liveDoing", "liveClock", "thinking",
    "answer", "foot", "runBar", "input", "send", "toast", "topLabel", "brandMark", "rowTitle", "rowState",
    "rowNow", "rowDoing"].forEach(function (id) { el[id] = doc.getElementById(id); });

  var steps = $$(".step", el.tlLive);
  var liveRows = $(".timeline-rows", el.tlLive);
  var doneRows = $(".timeline-rows", el.tlDone);
  var results = steps.map(function (s) { var r = $(".result", s); return r ? r.textContent : ""; });
  // The live list holds the same rows as the finished one, so it takes its readings from there.
  var finished = $$(".step", el.tlDone);
  while (steps.length < finished.length) {
    var copy = finished[steps.length].cloneNode(true);
    liveRows.appendChild(copy);
    steps.push(copy);
  }
  var readings = finished.map(function (s) {
    return { meta: $(".step-meta", s).innerHTML, verb: $(".verb", s).textContent, target: $(".target", s).textContent };
  });
  var ORBS = ["weaving", "searching", "working", "weaving"];

  var QUESTION = "Find the Brandt invoice and tell me what I owe.";
  var answerTexts = [];
  var walker = doc.createTreeWalker(el.answer, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    if (walker.currentNode.nodeValue.trim()) answerTexts.push({ node: walker.currentNode, full: walker.currentNode.nodeValue });
  }
  var paragraphs = $$("p", el.answer);

  function show(node, on) { node.classList.toggle("pre", !on); }
  function setMark(state) {
    el.brandMark.setAttribute("class", "poiesis-mark stage-2 state-" + state);
    el.brandMark.setAttribute("aria-label", "Poiesis Agent: " + (state === "active" ? "working" : "resting"));
  }
  /** Everything outside the turn that says a run is going: the mark, the box, the sidebar row. */
  function setBusy(busy) {
    el.send.textContent = busy ? "■" : "↑";
    el.send.setAttribute("aria-label", busy ? "Stop generating" : "Send message");
    el.input.placeholder = busy ? "Tell me something while I work" : "Message Poiesis";
    show(el.runBar, busy);
    show(el.rowState, busy);
    show(el.rowNow, busy);
    setMark(busy ? "active" : "idle");
  }
  function setTitle(title) { el.topLabel.textContent = title; el.rowTitle.textContent = title; }
  function clock(s) { return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); }
  function toggleTimeline(tl, rows, open) {
    tl.classList.toggle("open", open);
    rows.style.display = open ? "" : "none";
    $(".timeline-head", tl).setAttribute("aria-expanded", String(open));
  }

  function reset() {
    show(el.empty, true);
    [el.stream, el.turnUser, el.run, el.tlLive, el.tlDone, el.thinking, el.answer, el.foot, el.toast]
      .forEach(function (n) { show(n, false); });
    steps.forEach(function (s) { show(s, false); });
    toggleTimeline(el.tlLive, liveRows, false);
    toggleTimeline(el.tlDone, doneRows, false);
    el.tlDone.classList.remove("arrived");
    answerTexts.forEach(function (t) { t.node.nodeValue = ""; });
    paragraphs.forEach(function (p, i) { p.style.display = i ? "none" : ""; });
    el.answer.className = "run-text streaming pre";
    el.toast.classList.remove("arrive");
    el.input.value = "";
    el.liveClock.textContent = "0:00";
    setBusy(false);
    setTitle("New chat");
  }

  /* The finished state: what the window rests on. */
  function settle() {
    show(el.empty, false);
    show(el.tlLive, false);
    [el.stream, el.turnUser, el.run, el.tlDone, el.answer, el.foot].forEach(function (n) { show(n, true); });
    toggleTimeline(el.tlDone, doneRows, false);
    answerTexts.forEach(function (t) { t.node.nodeValue = t.full; });
    paragraphs.forEach(function (p) { p.style.display = ""; });
    el.answer.className = "run-text";
    show(el.thinking, false);
    setBusy(false);
    setTitle("Brandt invoice");
    show(el.toast, true);
  }

  [[el.tlLive, liveRows], [el.tlDone, doneRows]].forEach(function (pair) {
    $(".timeline-head", pair[0]).addEventListener("click", function () {
      toggleTimeline(pair[0], pair[1], !pair[0].classList.contains("open"));
    });
  });
  el.toast.addEventListener("click", function (e) {
    if (e.target.closest("#undo")) show(el.toast, false);
  });

  /* ---------- the scripted run ---------- */
  var token = 0;
  var CANCEL = {};
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

  /** The words on the one live line, said in the timeline's head and under the chat in the sidebar. */
  function doing(text) {
    [el.liveDoing, el.rowDoing].forEach(function (n) {
      n.textContent = text;
    });
    el.liveDoing.classList.remove("in");
    void el.liveDoing.offsetWidth;
    el.liveDoing.classList.add("in");
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
      doing("Thinking");
      setTitle("Brandt invoice");
      show(el.empty, false);
      show(el.stream, true);
      show(el.turnUser, true);
      await sleep(500, mine);
      show(el.run, true);
      show(el.thinking, true);
      var started = Date.now();
      await sleep(900, mine);

      // Steps: one live line, which opens into its rows if you click it.
      show(el.thinking, false);
      show(el.tlLive, true);
      tick = setInterval(function () {
        if (mine === token) el.liveClock.textContent = clock(Math.floor((Date.now() - started) / 1000));
      }, 500);
      for (var i = 0; i < steps.length; i++) {
        var s = steps[i];
        var r = readings[i];
        show(s, true);
        s.className = "step running";
        $(".step-meta", s).innerHTML = "";
        doing(r.verb + " " + r.target);
        window.PoiesisOrbs.set(el.liveOrb, ORBS[i % ORBS.length]);
        await sleep(1150, mine);
        s.className = "step done";
        $(".step-meta", s).innerHTML = r.meta;
        await sleep(160, mine);
      }

      // The answer, as it arrives.
      doing("Writing the answer");
      window.PoiesisOrbs.set(el.liveOrb, "composing");
      await sleep(600, mine);
      answerTexts.forEach(function (t) { t.node.nodeValue = ""; });
      show(el.answer, true);
      await streamAnswer(15, mine);
      await sleep(350, mine);

      // Done: the line becomes a count, the toast says what it kept.
      clearInterval(tick);
      el.tlDone.classList.add("arrived");
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
    else if (d.type === "settle") { token += 1; clearInterval(tick); reset(); settle(); }
  });
  if (window.matchMedia("(prefers-color-scheme: dark)").matches) setMode("dark");

  // Rows start closed, as in the app.
  toggleTimeline(el.tlDone, doneRows, false);

  // Opened on its own, it plays itself; inside the site, the page says when.
  if (window.parent === window) play();
  else {
    reset();
    if (reduced) settle();
    window.parent.postMessage({ type: "ready" }, "*");
  }
})();
