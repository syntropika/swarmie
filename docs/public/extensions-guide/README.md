# Extension contribution guide

This is a static, English visual guide to the implemented v1 contribution contract. It documents all 15 supported slots, screens, panels, commands, lifecycle events, the scoped client API, and the runtime boundary. The linked screenshots use the real application with clearly labeled fixture workspace data and simulated dictation.

From the repository root:

```sh
npm --prefix docs ci
npm --prefix docs run dev
```

Open `/extensions-guide/index.html` on the documentation server. The Fumadocs version is available at `/docs/extensions/visual-guide/`. Guide links lead to setup instructions for the separate desktop contribution demo. The demo never captures microphone audio or contacts a transcription service. The desktop UI has a minimum width of 760 pixels.

`npm --prefix docs run build` includes this guide in the documentation's static output. To run the application demo locally, start `npm --prefix apps/desktop run dev:web` and open `/contributions-demo.html` on that server. Desktop builds include the runnable demo; documentation assets belong to this site.

`../extensions/example.ts` is a focused bundled client contribution. Register it with the workspace's `ContributionRegistry`; the returned disposer removes the contribution. The running demo uses the same action pattern in `contributionDemo.tsx`. Runtime extensions can optionally implement `RuntimeExtension::ui()` and continue exposing their existing commands and views.

Screenshot overlays are measured from elements marked `data-contribution-slot`. Refresh both captures and overlay coordinates when those surfaces change. Final guide images live in `../extensions`, shared with the Fumadocs guide; raw screenshots and operational capture scripts belong outside the repository.
