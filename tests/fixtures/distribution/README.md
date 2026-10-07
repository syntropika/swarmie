# Package and HTTPS staging acceptance

Run against an actual Linux package and a new external artifact directory:

```sh
python3 tests/fixtures/distribution/acceptance.py \
  --package /absolute/path/mpai-0.1.0-linux-x86_64 \
  --artifacts /absolute/external/path/fresh-run
```

The fixture installs the real native host, daemon and frontend into private test paths, checks exact payload hashes, runs the installed CLI, repeats installation, and removes it while preserving separate history. No live installation or service is changed.

HTTPS staging uses the public httpbin Base64 echo endpoint with explicitly synthetic, non-executable archive and manifest bytes in the URL. No repository content or credentials are sent. The production CLI must verify TLS, stage the exact archive, reject a wrong checksum, and leave no rejected archive or partial file. `--https-origin` selects another compatible HTTPS echo service. Service availability is an external prerequisite; an unavailable service is a failed acceptance run, not evidence of an updater defect.
