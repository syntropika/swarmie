# Local runtime bundle

This npm package contains one actual runtime executable for the host platform and architecture recorded in `native/target.json`. Produce a bundle with `packages/clients/tools/package_local.py`; the source directory alone is not installable as an executable runtime.

```sh
npm install /absolute/path/mpai-runtime-local-0.1.0.tgz
npx mpai --help
npx mpai serve --http 127.0.0.1:7457
```

The launcher forwards arguments, standard streams, exit status, and termination signals. It rejects a different host platform or architecture and downloads nothing. Install provider harnesses separately. This package is a local distribution artifact; no registry release, notarization, code signing, or cross-platform binary availability is implied.
