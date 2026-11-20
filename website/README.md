# Cyder website

The landing page for [Cyder](https://github.com/GabrielSantos23/Cyder), the native Spotify desktop app.

Next.js 16 (App Router), React 19, Tailwind CSS 4.

```sh
npm install
npm run dev      # http://localhost:3000
npm run check    # lint + typecheck + production build
```

- `src/app/page.tsx`: the page and its content
- `src/components/sites/cyder/`: scroll motion, live GitHub release data, the draggable physics pills
- `src/app/opengraph-image.tsx`: the share image, rendered at build time
- `public/sites/am-rs/`: screenshots, the hero video, icons and fonts

Download links point at the latest release's `cyder.exe`; the version, date and file size are read from the GitHub API in the browser.

## Credits

- Started from [ai-website-cloner-template](https://github.com/JCodesMore/ai-website-cloner-template) (MIT, © 2025 JCodesMore).
- Fonts: Anton, Inter and JetBrains Mono, under the SIL Open Font License.
- Artist photos come from Wikimedia Commons under CC BY / CC BY-SA; each is credited in the page footer (see `assets/artist-photo-credits.json`).
