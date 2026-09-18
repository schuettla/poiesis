/// <reference types="vite/client" />

/** Vite's `?raw` import suffix, which hands back a file's text as a string.
 * `CTX-4`'s equivalence gate reads its golden this way rather than through
 * `node:fs`, so the frontend keeps needing no Node type definitions. */
declare module "*?raw" {
  const content: string;
  export default content;
}
