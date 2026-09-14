# Security

Français : [SECURITY.fr.md](SECURITY.fr.md)

## Supported versions

The latest published release. In `0.x` there is no maintenance branch: a fix
ships in the next version.

## Reporting a vulnerability

Through a private security advisory on the GitHub repository, never a public
issue. Expect a reply within a few days.

## In scope

The library decodes bytes handed to it by the host that the host did not write:
maps, meshes, textures. That is the only real attack surface, and the one that
matters — all the more so because it runs inside the host's process, not its
own.

- A map or mesh that crashes the decoder, loops forever, or exhausts memory
  while loading.
- A buffer overflow, out-of-bounds read or integer overflow reachable from a
  malformed file.
- A gap between what `docs/abi.md` guarantees and what the code does: an entry
  point that writes past the declared buffer, or lets a panic escape to the
  caller.
- Anything that would execute code from loaded content — **nothing in the
  formats allows it, and that is an invariant**: a map holds only identifiers,
  positions and dimensions, no binary, no script, no file path.

## Out of scope

**A host that violates the ABI's preconditions.** Passing an invalid pointer, a
`stride` smaller than the width, an already-destroyed handle: those conditions
are documented, and honouring them is the caller's responsibility. That is the
nature of a C boundary, not a defect in the library.

Editing your own files to get a different image. There is nothing here to
protect against its owner.
