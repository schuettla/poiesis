# Poiesis website

A static page. Open `index.html`. There is no build step for the site itself.

## The hero window

The window in the hero is the real app, not a drawing of it.

- `app/index.html` uses the same markup and class names as the app's shell
  (top bar, rail, chat, composer, Workbench).
- `app/app.css` is the app's own stylesheets, copied unchanged.
- `app/orb.js` is the app's activity orb (`thinking-orbs`, MIT).
- `app/demo.js` plays one scripted run. `index.html` shows it in an iframe.

`app/app.css` and `app/orb.js` are generated. After the app's styles change, run
this from the repo root:

```sh
node website/sync-app.mjs
```

If the app's markup changes (a new class, a moved panel), update
`app/index.html` by hand to match.
