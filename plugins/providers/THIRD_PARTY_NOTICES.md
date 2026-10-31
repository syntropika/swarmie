# Third-party notices

## Pi OAuth protocol port

The native OAuth protocol code in `src/` includes a Rust adaptation of the
following MIT-licensed Pi source files at revision
`1de2d7adeacfd1b065d9df091997d44c43120ade`:

- `packages/ai/src/auth/oauth/openai-chatgpt.ts`
- `packages/ai/src/auth/oauth/github-copilot.ts`
- `packages/ai/src/auth/oauth/kimi-coding.ts`
- `packages/ai/src/auth/oauth/device-code.ts`
- `packages/ai/src/auth/oauth/xai.ts`

Source: https://github.com/earendil-works/pi/tree/1de2d7adeacfd1b065d9df091997d44c43120ade/packages/ai/src/auth/oauth

This adaptation covers provider authorization, token exchange, refresh, and device
polling semantics. Swarmie's Tembra contract integration, principal binding,
credential storage, connection administration, and lifecycle ownership are separate
application code. The public upstream client registrations are not represented as
Swarmie-owned OAuth registrations.

### License

MIT License

Copyright (c) 2025 Mario Zechner

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.