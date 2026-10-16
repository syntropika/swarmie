# Git extension

The optional `swarmie-git` extension provides repository registration, managed worktrees, turn checkpoints, review, staging, commits, ordinary pushes, and GitHub CLI operations. Register it through the public runtime builder and discover commands with `extension/list`. The `git-review` and `git-workflow` view descriptors expose the same headless commands.

Open a repository with `project.open {path}`. An unborn repository is supported for its initial commit; creating secondary worktrees requires an existing commit. Other commands use its returned `projectId`; worktree operations additionally use a discovered `worktreeId`.

| Command | Additional arguments | Behavior |
| --- | --- | --- |
| `worktree.list` | None | Registered Git worktrees |
| `worktree.create` | New `branch`, optional `base`, `setup` | Create a managed secondary checkout; retain it if setup fails |
| `worktree.remove` | `worktreeId` | Remove an unlocked, clean managed checkout and retain its branch |
| `worktree.cleanup` | Explicit `worktreeIds` array | Per-worktree cleanup receipts; failures preserve the checkout |
| `worktree.setup.configure` | `commands: [{program,args}]`, optional `submodules` | Persist explicit project setup configuration |
| `worktree.setup.run` | `worktreeId` | Run configured setup in a managed secondary checkout |
| `branches` | None | Local and remote branch names |
| `status` | Optional `worktreeId` | Branch, dirty state, native status entries, remotes, ahead/behind counts |
| `stage`, `unstage` | `worktreeId`, literal relative `paths` | Update the index |
| `commit` | `message`, optional `worktreeId` | Commit already staged changes using the exact multiline message |
| `push` | Optional `remote` (default `origin`), `worktreeId` | Push the checked-out branch and set its upstream; no force push |
| `text.prompt` | `kind: commit/pr/branch`, optional `instructions`, `worktreeId`; required `base` for PR | Return a reusable generation prompt; the client chooses its agent |
| `review` | `worktreeId`, optional `scope`, `base`, `file`, `sessionId`, `turnId` | Bounded native diff and file list |
| `checkpoints` | `sessionId` only | Saved before/after turn tree identifiers and checkout paths |
| `checkpoint.restore` | `worktreeId`, `sessionId`, `turnId`, optional `boundary: before/after`, `paths` | Explicitly restore a retained tree into a clean managed secondary checkout |

Setup accepts at most eight direct programs with argument arrays. No shell is inserted automatically. Each command has a 120-second deadline and bounded output. Optional recursive submodule initialization uses native Git configuration and authentication. Native local-file protocol restrictions remain effective; a user may explicitly configure a different Git invocation for a trusted local repository. Setup failures report a message and preserve the new worktree. Cleanup refuses uncommitted and nonignored untracked changes, locked worktrees, main checkouts, foreign checkouts, and checkouts containing any open runtime session, including idle native processes. Closing the session releases that guard. Initialized clean submodules are deinitialized without force. Git requires a narrowly scoped force-removal flag for a checkout containing submodules even after deinitialization; the extension uses it only after these ownership, lock and cleanliness checks. Submodule edits are never discarded.

Commit generation uses staged changes. PR generation resolves the supplied base to a commit and compares `base...HEAD`, so committed branch changes remain available after an empty index. Diff context is capped at 96 KiB with a `truncated` receipt. Generated text is a proposal; publishing commands require a separate invocation.

## GitHub operations

GitHub features require an installed `gh` CLI and its configured authentication. `integration.status` returns executable availability, version and authentication readiness without exposing credentials. Operations use direct arguments and private temporary body files, preserving literal text and multiline bodies. No operation merges a pull request.

All PR commands accept optional positive `number`; omission selects the current branch's PR. They also accept optional `worktreeId`.

| Command | Additional arguments | Result |
| --- | --- | --- |
| `pr.create` | `title`, `body`, optional `base`, `head` | Create a draft PR; return `updated`, `url`, `draft` |
| `pr.status` | None | Native PR metadata, files, review decision and checks |
| `pr.checks` | None | Check array with name, state, bucket and link; pending/failing checks remain readable |
| `pr.comments` | None | Native comments and reviews |
| `pr.comment` | `body` | Publish a comment |
| `pr.review` | `event: approve/request_changes/comment`, `body` (optional for approval) | Submit the selected review |
| `pr.viewed` | Literal relative `path`, optional `viewed` (default true) | Mark or unmark a viewed file |
| `pr.viewed.list` | None | Native `files` with `path` and `viewerViewedState`, plus `truncated` |

Viewed-file synchronization uses GitHub GraphQL with the PR's actual node identifier and bounded pagination (800 files). Checks support modern `gh pr checks --json`; older CLIs fall back to native `statusCheckRollup`. Network/authentication failures are actionable command errors. Tests use independent CLI peers and local bare repositories, without publishing to a live service.

## Review and restoration

`review` accepts `scope: working/staged/unstaged/base/turn`, optional literal relative `file`, and a `base` revision (default `HEAD`). Base references are resolved before comparison. Turn review requires a completed checkpoint, `sessionId`, and `turnId`. Results contain `files`, `diff`, `base`, `scope`, `truncated`, and `filesTruncated`; patches are capped at 96 KiB and lists at 128 entries. Binary files and symlink destinations are described without traversing their contents.

Before an accepted prompt, the extension captures the checkout with a separate index. Canonical turn events retain before/after trees without modifying the user's index. Metadata lives under the managed root's `checkpoints/`; `refs/swarmie/checkpoints/` retain trees across Git garbage collection. Captures include tracked files and nonignored untracked files; ignored files and nested submodule contents are outside that comparison. Failed capture does not prevent the prompt.

Restoration is explicit and never triggered by opening a review. `checkpoint.restore` requires a clean, unlocked, extension-managed secondary worktree belonging to the checkpoint. Every logical agent session using that checkout or a nested directory must be closed first, including idle sessions. `boundary` defaults to `before`; `after` requires a completed checkpoint. Optional nonempty `paths` selects up to 128 literal relative files; omission restores the captured tree. Git updates the worktree and index while retaining the current branch and commit. The result is `{restored:true,boundary,base,paths,branchChanged:false}`. It does not change agent history or perform provider rollback. Repeated restoration requires preserving or committing the resulting staged changes first.
