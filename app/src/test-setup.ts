// The app reads document.baseURI (config.ts), location and history at import or call time; tests run in node, so give them minimal stand-ins.
const loc = { hash: '' }
Object.assign(globalThis, {
  document: { baseURI: 'http://localhost/' },
  location: loc,
  history: {
    replaceState: (_s: unknown, _t: string, url: string) => {
      loc.hash = url.replace(/^[^#]*/, '')
    },
  },
})
