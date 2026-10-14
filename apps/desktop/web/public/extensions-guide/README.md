# Extension contribution guide

This is a static, English visual guide to the implemented v1 contribution contract. It documents all 15 supported slots, screens, panels, commands, lifecycle events, the scoped client API, and the runtime boundary. The linked screenshots use the real application with clearly labeled fixture workspace data and simulated dictation.

From the repository root:

```sh
npm --prefix apps/desktop install
npm --prefix apps/desktop run dev:web
```

Open `/extensions-guide/index.html` on the development server shown by Vite. Open `/contributions-demo.html` to run the contribution example without a runtime. The demo never captures microphone audio or contacts a transcription service. The desktop UI has a minimum width of 760 pixels.

`npm --prefix apps/desktop run build:web` includes both the guide and the separate demo entry in the web distribution. Use Vite's preview server or any static HTTP server to review the built files.

`example.ts` is a focused bundled client contribution. Register it with the workspace's `ContributionRegistry`; the returned disposer removes the contribution. The running demo uses the same action pattern in `contributionDemo.tsx`. Runtime extensions can optionally implement `RuntimeExtension::ui()` and continue exposing their existing commands and views.

Screenshot overlays are measured from elements marked `data-contribution-slot`. Refresh both captures and overlay coordinates when those surfaces change. Final guide images live in `assets`; raw screenshots and operational capture scripts belong outside the repository.
