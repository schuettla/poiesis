# Poiesis website

A static site. Open `index.html`. There is no build step for the site itself.

- `index.html` is the short pitch: what makes Poiesis different, an outline of
  the features, and the downloads.
- `features.html` holds every feature in detail. A new feature goes there. The
  overview changes only when the pitch does.

Both pages share `styles.css` and `script.js`.

## The hero window

The window in the hero is the real app, not a drawing of it.

- `app/index.html` holds the markup the app renders for its shell (top bar,
  rail, chat, composer, Workbench), captured from the running app.
- `app/app.css` is the app's own stylesheets, copied unchanged.
- `app/orb.js` is the app's activity orb (`thinking-orbs`, MIT).
- `app/demo.js` plays one scripted run. `index.html` shows it in an iframe.

## The phone screens

The three phones in the phone section of `features.html` work the same way.

- `app/phone.html` holds the markup of three screens of `mobile/`: home, a
  question to answer, and voice. The address picks one (`#home`, `#ask`,
  `#voice`).
- `app/phone.css` is the phone app's own stylesheets, copied unchanged.
- `app/phone-demo.css` and `app/phone-demo.js` add the status bar and the
  lighting.

## Keeping them current

`app/app.css`, `app/phone.css` and `app/orb.js` are generated. After the styles
change, run this from the repo root:

```sh
node website/sync-app.mjs
```

If the markup changes (a new class, a moved panel), update `app/index.html` or
`app/phone.html` to match.
