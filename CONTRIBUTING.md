# Contributing

Français : [CONTRIBUTING.fr.md](CONTRIBUTING.fr.md)

## How this project is written

The code is written in pair with an assistant, under the rules of this
repository. That is not a footnote: the method is the project's second subject,
and the way the rules are laid down follows from it.

- **Rules come before code**, they are not inferred from it.
  [`docs/abi.md`](docs/abi.md) is authoritative, and a disagreement between the
  document and the code is a defect in the code. Everything under `docs/` is in
  French.
- **A decision carries what it rules out.** Every trade-off keeps the reasons
  for the rejected options, so it can be reopened without being replayed.
- **What can be measured is measured.** On a rasteriser, reasoning is often
  wrong and the hash decides.
- **Commit messages do not tell how the change was made.** They say what
  changes and why — the rest is in the diff.

None of this applies differently to an outside contribution: same checks, same
style rules. Only the bilingual rule is waived — see "Language".

## Before writing code

Open an issue first for anything beyond a fix. The boundary contract is written
in [`docs/abi.md`](docs/abi.md); changing it is a discussion, not a patch.

## What is discussed before it is written

A pull request that touches one of these paths without prior discussion will be
sent back to an issue, whatever its quality — not on principle, but because
these are the places where one change tips others over.

- **[`docs/abi.md`](docs/abi.md)** is authoritative. The code conforms to it, so
  changing one line of it changes what the code must do.
- **`include/screengine.h`** is a public contract: a host compiled against it six
  months ago must still link. Functions are added, a published signature is
  never modified. The file is **generated** — a manual edit is a defect, and CI
  rejects it.
- **The map and mesh format** is a public contract in the same way: a level
  produced today must load tomorrow. An added field is optional; a removed or
  renamed field breaks everything in circulation.
- **`crates/screengine-conformance/references/`** decides what rendering must
  produce. A hash changed in the same commit as the code that changes it cannot
  be reviewed: the change and the reference update are two commits.
- **`.github/workflows/`** decides what is checked. Branch protection requires
  checks by name, not by content: a modified workflow can turn green a check
  that no longer verifies anything.

Everything else — code, tests, accompanying documentation — can be proposed
directly.

## What a contribution is judged on

Code conventions and the testing doctrine are in [`docs/rust.md`](docs/rust.md).
What follows is the enforceable summary.

- `make fmt && make lint && make test && make conform && make nostd && make header-verif && make audit` pass.
- Every declaration is documented. Comments say *why*; they never paraphrase
  the next line.
- No banners, no decorative emoji, neither in code nor in commit messages.
- **The core stays `no_std` and dependency-free.** A `use std::` added to
  `crates/screengine` is a defect, even if it compiles on its author's machine.
- **No per-frame allocation.** Everything is allocated when the context is
  created; working buffers are reused through `clear`, never reallocated.
- **No libm calls.** Trigonometry and inverse square root go through the core's
  tables. An `f32::sin` introduced makes hashes diverge across platforms, and
  the gap only shows on the target you do not build yourself.
- **`unsafe` only in `screengine-ffi` and SIMD paths**, with a comment naming
  the invariant the caller upholds.
- **Every FFI entry point is wrapped in `catch_unwind`.** A panic crossing the
  boundary is undefined behaviour, not a clean crash.
- Nothing in `crates/screengine` imports `screengine-ffi`. Runners are headless;
  a test that needs a window has no place in the default suite.
- An added dependency goes into `THIRD-PARTY-NOTICES` — and in the core, it does
  not go in at all.

## Delivery

**One change, one branch, one commit.** The branch starts from an up-to-date
`master` and is named `<type>/<topic>`, where the type is the conventional
prefix of its commit: `feat/`, `fix/`, `docs/`, `chore/`, `test/`, `refactor/`.
Do not chain two changes on the same branch — each must stay reviewable and
revertible on its own.

It goes back into `master` **through a pull request**, never through a local
merge: the PR is what records what was delivered, and its merge is what deletes
the branch on both sides.

**Check before pushing, not after:**

```
make fmt && make lint && make test && make conform && make nostd && make header-verif && make audit
```

**The list is fixed and runs in full**, never trimmed to what the change you
just wrote touches. Composing your own list means checking only what you already
have in mind, and the defect is elsewhere by construction: had it been where you
were looking, you would have seen it while writing. **The check that finds it is
the one you had no reason to run.**

`make nostd` and `make header-verif` are the two you are tempted to skip because
they always pass. They are also the two whose failure costs the most: found at
porting time, a forgotten `use std::` already has three weeks of code built on
it.

One more check is added as soon as a host is touched:

```
make hosts
```

`cargo audit` queries its advisory database **live**: a job green in the morning
can be red in the afternoon on exactly the same code. Do not rely on CI alone,
which validates once the branch has already been pushed.

**The `CHANGELOG` section ships with the change**, not at tag time: it is
reviewed in the pull request, which is when it matters. The release takes its
name and notes from it, and a missing section stops the release.

**Documentation ships with the change.** Before committing, check what the
change makes wrong elsewhere: the status stated in the README, a clause of
[`docs/abi.md`](docs/abi.md), a step of [`ROADMAP.md`](ROADMAP.md) (French).

**A message says what changes and why**, in a few lines. The default is the
title alone: a body exists only if it carries something the title does not say
and the diff does not show.

## Fixing a vulnerability without creating another

Do not adopt a published version **the same day**, even a fix. Look for the
oldest one that is enough:

```
cargo search <crate> --limit 1
cargo tree -i <crate>
```

A version released within the hour is the typical profile of a compromised
maintainer account.

A pin is explained: a dependency held below the latest available carries an
end-of-line comment saying why, and **when to remove it**.

## Three numbers not to confuse

| Number | Where | What it tracks |
|---|---|---|
| repository version | git tag | the library |
| `SCG_ABI_VERSION` | `include/screengine.h` and `scg_abi_version()` | the C boundary |
| `version_format` | every map and every mesh | the file format |

The last two do not follow SemVer. They are integers: adding a function to the
ABI or an optional field to a format does not increment them, everything else
does, and an increment of `version_format` requires writing the migration of
existing files.

`SCG_ABI_VERSION` exists so that a binding cleanly refuses a library that is too
old, rather than linking and rendering garbage.

The repository follows SemVer with the zero clause, defined in
[`CHANGELOG.md`](CHANGELOG.md): **in `0.x`, nothing is guaranteed.** The minor
number marks a step of [`ROADMAP.md`](ROADMAP.md), not an API break; everything
else accumulates as patches. Direct consequence: **the number warns of
nothing**, and it is the release notes that must say what a binding author has
to rework.

## Language

**Identifiers are in English** — directories, files, modules, types, functions,
fields. **Documentation is in French**: module docs, item docs, comments, error
messages. The API reads in English because it is code; the reasoning reads in
French because it is thought.

**One exception, and a structural one: documentation of items exported through
FFI.** `cbindgen` copies it into `include/screengine.h`, read by binding authors
who do not speak French. Those docstrings are in English, and they are the only
ones.

Commit messages in French first, English second, in a single text separated by
`***`. Never `---`: `git am` treats it as a patch separator and truncates
everything after it.

Contributions in English are welcome and are not subject to the bilingual rule.

## Bindings

Bindings to other languages live in separate repositories, with their own
release pace, and **contain no logic** — only type conversion.

A binding that computes something is a binding that will diverge: the same
computation will soon exist in Go and in Python, with two behaviours. What must
be shared moves up into the core.

A binding is admitted to the official list when it passes the conformance suite
and its hashes are identical to those of the native scalar path.
