/* Poiesis website. No dependencies. Everything here is progressive: with
   scripts off the page still reads, and the hero shows its finished state. */
(function () {
  "use strict";

  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var root = document.documentElement;
  var $ = function (sel, el) { return (el || document).querySelector(sel); };
  var $$ = function (sel, el) { return Array.prototype.slice.call((el || document).querySelectorAll(sel)); };

  /* ---------- the mark: membrane, nucleus, up to three orbit dots ---------- */
  var ORBITS = [
    [12, 4],
    [18.93, 16],
    [5.07, 16],
  ];
  function drawMark(svg, stage) {
    var dots = ORBITS.slice(0, stage)
      .map(function (o) { return '<circle class="mark-orbit" cx="' + o[0] + '" cy="' + o[1] + '" r="1.5"/>'; })
      .join("");
    svg.innerHTML =
      '<circle class="mark-membrane" cx="12" cy="12" r="8" fill="none"/>' +
      '<g class="mark-orbits">' + dots + "</g>" +
      '<circle class="mark-nucleus" cx="12" cy="12" r="3"/>';
  }
  function stageOf(svg) {
    var m = /stage-(\d)/.exec(svg.getAttribute("class") || "");
    return m ? +m[1] : 0;
  }
  $$("svg.poiesis-mark").forEach(function (svg) { drawMark(svg, stageOf(svg)); });

  /* ---------- lighting: Daylight and Backlit ---------- */
  var modeBtns = $$("[data-mode-btn]");
  function setMode(mode, remember) {
    root.setAttribute("data-mode", mode);
    modeBtns.forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.modeBtn === mode)); });
    var meta = $$('meta[name="theme-color"]');
    meta.forEach(function (m) { m.setAttribute("content", mode === "dark" ? "#0d100e" : "#f3f5f3"); m.removeAttribute("media"); });
    if (remember) {
      try { localStorage.setItem("poiesis-mode", mode); } catch (e) {}
    }
    if (typeof sendMode === "function") sendMode();
  }
  setMode(root.getAttribute("data-mode") || "light", false);
  modeBtns.forEach(function (b) {
    b.addEventListener("click", function () {
      if (b.dataset.modeBtn === root.getAttribute("data-mode")) return;
      root.classList.add("mode-swap");
      setMode(b.dataset.modeBtn, true);
      setTimeout(function () { root.classList.remove("mode-swap"); }, 420);
    });
  });

  /* ---------- reveal on scroll ---------- */
  var reveals = $$("[data-reveal]");
  if ("IntersectionObserver" in window && !reduce) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.05 });
    reveals.forEach(function (el) { io.observe(el); });
  } else {
    reveals.forEach(function (el) { el.classList.add("in"); });
  }

  /* ---------- hero: the real window ----------
     The window is the app itself (app/index.html) in an iframe at the app's
     default size, scaled to the column. The page tells it the lighting and
     when to start; it plays one run, then rests. */
  var shot = $("#shot");
  var frame = $("#appFrame");
  var shotInner = $("#shotFrame");
  var APP_W = 1180;
  var APP_H = 800;
  var ready = false;
  var visible = false;
  var played = false;

  function post(msg) {
    try { frame.contentWindow.postMessage(msg, "*"); } catch (e) {}
  }
  function sendMode() {
    var msg = { type: "mode", mode: root.getAttribute("data-mode") };
    if (frame) post(msg);
    (phones || []).forEach(function (f) {
      try { f.contentWindow.postMessage(msg, "*"); } catch (e) {}
    });
  }
  function maybePlay() {
    if (ready && visible && !played) { played = true; post({ type: "play" }); }
  }
  function fit() {
    var scale = shot.clientWidth / APP_W;
    shotInner.style.transform = "scale(" + scale + ")";
    shot.style.height = Math.round(APP_H * scale) + "px";
    shot.classList.add("fit");
  }
  function onReady() {
    if (ready) return;
    ready = true;
    sendMode();
    maybePlay();
  }

  if (shot && frame) {
    fit();
    if ("ResizeObserver" in window) new ResizeObserver(fit).observe(shot);
    else window.addEventListener("resize", fit);

    window.addEventListener("message", function (e) {
      if (e.source === frame.contentWindow && e.data && e.data.type === "ready") onReady();
    });
    frame.addEventListener("load", onReady);
    setTimeout(onReady, 2500);

    if ("IntersectionObserver" in window) {
      var heroIo = new IntersectionObserver(function (es) {
        es.forEach(function (e) { if (e.isIntersecting) { visible = true; maybePlay(); heroIo.disconnect(); } });
      }, { threshold: 0.3 });
      heroIo.observe(shot);
    } else {
      visible = true;
    }

    var replay = $("#replay");
    if (replay) {
      if (reduce) replay.hidden = true;
      else replay.addEventListener("click", function () { played = true; post({ type: "replay" }); });
    }
  }

  /* ---------- the phone screens ----------
     Each frame holds one screen of the companion app (app/phone.html) at a
     phone's size, scaled to its column and told the lighting. */
  var phones = $$(".phone-screen iframe");
  if (phones.length) {
    var fitPhones = function () {
      phones.forEach(function (f) { f.style.transform = "scale(" + f.parentNode.clientWidth / 390 + ")"; });
    };
    fitPhones();
    if ("ResizeObserver" in window) new ResizeObserver(fitPhones).observe(phones[0].parentNode);
    else window.addEventListener("resize", fitPhones);
    window.addEventListener("message", function (e) {
      if (e.data && e.data.type === "phone-ready") sendMode();
    });
    phones.forEach(function (f) { f.addEventListener("load", sendMode); });
    sendMode();
  }

  /* ---------- the mark, up close ---------- */
  var lab = $("#markLab");
  if (lab) {
    var labMark = $("#labMark");
    var labSay = $("#labSay");
    var state = "idle";
    var stage = 1;
    var STATE_TEXT = {
      idle: "<b>Resting.</b> Nothing is happening. The membrane sits quiet.",
      active: "<b>Working.</b> The centre breathes while the agent is busy on a task.",
      reflecting: "<b>Reflecting.</b> The dots go round while it reads back over a finished chat.",
      healing: "<b>Recovering.</b> The membrane flickers while it repairs something.",
    };
    var STAGE_TEXT = [
      "It knows nothing yet, so the membrane is only a dotted line.",
      "It has learned a few things. The first dot has been earned.",
      "A fuller picture of you. A second dot.",
      "Fifty or more notes, facts and skills. All three dots. Growth is slow on purpose.",
    ];
    var renderLab = function () {
      drawMark(labMark, stage);
      labMark.setAttribute("class", "poiesis-mark stage-" + stage + " state-" + state);
      labSay.innerHTML = STATE_TEXT[state] + " " + STAGE_TEXT[stage];
      $$("[data-state]", lab).forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.state === state)); });
      $$("[data-stage]", lab).forEach(function (b) { b.setAttribute("aria-pressed", String(+b.dataset.stage === stage)); });
    };
    lab.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      if (b.dataset.state) state = b.dataset.state;
      if (b.dataset.stage) stage = +b.dataset.stage;
      renderLab();
    });
    renderLab();
  }

  /* ---------- memory files ---------- */
  var FILES = {
    index: {
      path: "memory/MEMORY.md",
      who: "the agent writes this",
      note: "The index. A short version of it goes into every conversation.",
      body:
        "# Memory\n\n" +
        "- facts/short-answers.md\n  prefers short answers\n" +
        "- facts/invoices-by-year.md\n  files invoices by year\n" +
        "- lessons/ask-before-send.md\n  ask before sending mail\n",
    },
    soul: {
      path: "memory/SOUL.md",
      who: "only you write this",
      note: "Your standing instructions. The agent can propose an edit, and you accept or decline.",
      body:
        "# Standing instructions\n\n" +
        "- Answer in the language I write in.\n" +
        "- Never send mail without asking me.\n" +
        "- Say when you did not check something.\n",
    },
    profile: {
      path: "memory/PROFILE.md",
      who: "the agent writes this",
      note: "The agent's own picture of how you like to be worked with. It can be rebuilt, and the rebuild has an Undo.",
      body:
        "# How to work with this person\n\n" +
        "Likes short answers and reads on a phone.\n" +
        "Wants a plan first for big jobs.\n" +
        "Files invoices by year.\n",
    },
    fact: {
      path: "memory/facts/short-answers.md",
      who: "saved by the agent, with an Undo",
      note: "One fact per file. Delete the file and the agent forgets it.",
      body:
        "---\nname: short-answers\ndescription: prefers short answers\n---\n\n" +
        "Keep answers under one screen.\nOffer more detail only when asked.\n",
    },
    lesson: {
      path: "memory/lessons/ask-before-send.md",
      who: "learned from a mistake",
      note: "A lesson passed a second check before it was allowed in.",
      body:
        "---\nname: ask-before-send\ndescription: ask before sending mail\n---\n\n" +
        "A draft once went out in the wrong tone.\nShow the draft first. Send only after a yes.\n",
    },
  };
  var tree = $("#files");
  if (tree) {
    var show = function (key) {
      var f = FILES[key];
      $("#filePath").textContent = f.path;
      $("#fileWho").textContent = f.who;
      $("#fileBody").textContent = f.body;
      $("#fileNote").textContent = f.note;
      $$("[data-file]", tree).forEach(function (b) { b.setAttribute("aria-selected", String(b.dataset.file === key)); });
    };
    tree.addEventListener("click", function (e) {
      var b = e.target.closest("[data-file]");
      if (b) show(b.dataset.file);
    });
    // Arrow keys move through the list, as in a tablist.
    tree.addEventListener("keydown", function (e) {
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      var tabs = $$("[data-file]", tree);
      var i = tabs.indexOf(document.activeElement);
      if (i < 0) return;
      e.preventDefault();
      var next = tabs[(i + (e.key === "ArrowDown" ? 1 : tabs.length - 1)) % tabs.length];
      next.focus();
      show(next.dataset.file);
    });
    show("index");
  }

  /* ---------- autonomy demo ---------- */
  var box = $("#autonomy");
  if (box) {
    var out = $("#outcome");
    var DIAMOND = '<i class="mk"></i>';
    var VIEWS = {
      auto: function () {
        return (
          '<div class="toast">' + DIAMOND +
          '<span class="tx">I\'ll remember that: you use metric units</span>' +
          '<button class="undo" type="button" data-act="undo">Undo</button></div>'
        );
      },
      ask: function () {
        return (
          '<div class="proposal"><div>I would like to remember: <strong>you use metric units.</strong></div>' +
          '<div class="acts"><button class="btn btn-primary btn-sm" type="button" data-act="keep">Keep it</button>' +
          '<button class="btn btn-secondary btn-sm" type="button" data-act="decline">Not now</button></div></div>'
        );
      },
      off: function () {
        return '<div class="quiet-note">Nothing is written to memory. The agent can still tell you what it noticed.</div>';
      },
    };
    var current = "auto";
    var render = function (mode) {
      current = mode;
      out.innerHTML = VIEWS[mode]();
      $$("[data-auto]", box).forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.auto === mode)); });
    };
    box.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      if (b.dataset.auto) return render(b.dataset.auto);
      var act = b.dataset.act;
      if (act === "undo") {
        out.innerHTML = '<div class="quiet-note"><span class="undone">I\'ll remember that: you use metric units</span><br>Undone. It is forgotten, and the file is gone.</div>';
      } else if (act === "keep") {
        out.innerHTML = '<div class="toast">' + DIAMOND + '<span class="tx">I\'ll remember that: you use metric units</span>' +
          '<button class="undo" type="button" data-act="undo">Undo</button></div>';
      } else if (act === "decline") {
        out.innerHTML = '<div class="quiet-note">Declined. Nothing was saved.</div>';
      }
    });
    render(current);
  }

  /* ---------- fleet: steer and stop ---------- */
  var fleet = $("#fleet");
  if (fleet) {
    fleet.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      var row = b.closest(".fleet-row");
      if (!row) return;
      if (b.hasAttribute("data-steer")) {
        var existing = row.nextElementSibling && row.nextElementSibling.classList.contains("steer") ? row.nextElementSibling : null;
        if (existing) { existing.remove(); return; }
        var f = document.createElement("form");
        f.className = "steer";
        f.innerHTML = '<input type="text" aria-label="Tell this agent something" placeholder="Tell this agent something" />';
        row.parentNode.insertBefore(f, row.nextElementSibling);
        $("input", f).focus();
        f.addEventListener("submit", function (ev) {
          ev.preventDefault();
          var p = document.createElement("p");
          p.className = "steer-note";
          p.textContent = "Sent. It will pick this up after the step it is on.";
          f.replaceWith(p);
        });
        f.addEventListener("keydown", function (ev) { if (ev.key === "Escape") f.remove(); });
      } else if (b.hasAttribute("data-stop")) {
        var lead = $(".lead", row);
        lead.innerHTML = '<i class="sig" style="border: 1px solid var(--ink-faint)"></i>';
        $(".doing", row).textContent = "stopped, I kept what it had · 4 steps";
        var clock = $(".clock", row);
        if (clock) clock.textContent = "";
        var acts = $(".acts", row);
        acts.innerHTML = '<button type="button">Open</button><button type="button">Report</button>';
      }
    });
  }

  /* ---------- features page: mark the section being read ---------- */
  var toc = $(".toc");
  if (toc) {
    var links = $$("a", toc);
    var sections = links.map(function (a) { return $(a.getAttribute("href")); });
    var current = null;
    var spy = function () {
      var at = 0;
      sections.forEach(function (s, i) { if (s.getBoundingClientRect().top <= 160) at = i; });
      // The last sections may be too short to reach the top.
      if (window.innerHeight + window.scrollY >= root.scrollHeight - 4) at = links.length - 1;
      if (links[at] === current) return;
      if (current) current.removeAttribute("aria-current");
      current = links[at];
      current.setAttribute("aria-current", "true");
      // Keep it in sight in the contents, whichever way they run.
      var a = current.getBoundingClientRect();
      var box = toc.getBoundingClientRect();
      if (a.top < box.top || a.bottom > box.bottom) toc.scrollTop += a.top - box.top - box.height / 2;
      if (a.left < box.left || a.right > box.right) toc.scrollLeft += a.left - box.left - 18;
    };
    window.addEventListener("scroll", spy, { passive: true });
    spy();
  }

  /* ---------- downloads ----------
     The file names carry the version, so the buttons are filled from the
     latest published release. Until one is published (or if GitHub doesn't
     answer) every button stays on the releases page. The visitor's own
     system only gets a quieter border; every Download button leads to all three. */
  var dl = $(".dl");
  if (dl) {
    var MATCH = {
      exe: /-setup\.exe$/i,
      msi: /\.msi$/i,
      dmg: /\.dmg$/i,
      appimage: /\.appimage$/i,
      deb: /\.deb$/i,
    };
    var ua = navigator.userAgent || "";
    var os = /Android|iPhone|iPad/.test(ua) ? null
      : /Mac/.test(ua) ? "macos"
      : /Win/.test(ua) ? "windows"
      : /Linux|X11/.test(ua) ? "linux"
      : null;

    var card = os && $('[data-os="' + os + '"]', dl);
    if (card) card.classList.add("here");

    fetch("https://api.github.com/repos/schuettla/poiesis/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
    })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (release) {
        if (!release || !release.assets) return;
        var url = {};
        release.assets.forEach(function (asset) {
          Object.keys(MATCH).forEach(function (kind) {
            if (!url[kind] && MATCH[kind].test(asset.name)) url[kind] = asset.browser_download_url;
          });
        });
        $$("[data-asset]", dl).forEach(function (a) {
          if (url[a.dataset.asset]) a.href = url[a.dataset.asset];
        });
      })
      .catch(function () {});
  }
})();
