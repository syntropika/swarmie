# Extension contribution guide

This is a static, English visual guide to the implemented v1 contribution contract. It documents all 15 supported slots, screens, panels, commands, lifecycle events, the scoped client API, and the runtime boundary. The linked screenshots use the real application with clearly labeled fixture workspace data and simulated dictation.

From the repository root:

```sh
npm --prefix apps/docs ci
npm --prefix apps/docs run dev
```

Open `/extensions-guide/index.html` on the documentation server. The Fumadocs version is available at `/docs/extensions/visual-guide/`. Guide links lead to setup instructions for the separate desktop contribution demo. The demo never captures microphone audio or contacts a transcription service. The desktop UI has a minimum width of 760 pixels.

`npm --prefix apps/docs run build` includes this guide in the documentation's static output. To run the demo locally, start `npm --prefix apps/desktop run dev:demo` and open `/demo/index.html` on that server. `build:demo` writes a separate `apps/desktop/demo-dist` distribution; production desktop builds exclude the demo and documentation assets.

`../extensions/example.ts` is a focused bundled client contribution. Register it with the workspace's `ContributionRegistry`; the returned disposer removes the contribution. The running demo uses the same action pattern in `apps/desktop/web/demo/contributions.tsx`. Runtime extensions can optionally implement `RuntimeExtension::ui()` and continue exposing their existing commands and views.

Screenshot overlays are measured from elements marked `data-contribution-slot`. Refresh both captures and overlay coordinates when those surfaces change. Final guide images live in `../extensions`, shared with the Fumadocs guide; raw screenshots and operational capture scripts belong outside the repository.
