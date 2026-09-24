# Security

Français : [SECURITY.fr.md](SECURITY.fr.md)

## Supported versions

The latest published release. In `0.x` there is no maintenance branch: a fix
ships in the next version.

## Reporting a vulnerability

Through a private security advisory on the GitHub repository, never a public
issue. Expect a reply within a few days.

## In scope

The library decodes bytes handed to it by the host that the host did not write.
That is the only real attack surface, and the one that matters — all the more so
because it runs inside the host's process, not its own.

**Today that means texture pixel blocks**, and them alone. Map and mesh formats
do not exist yet: they are step 4 of the roadmap, and this section will name
them when they land rather than promise them early — a scope that claims more
than the code does wastes a reporter's time.

- A texture description or pixel block that crashes the loader, reads out of
  bounds, or overflows an integer on its way to an allocation size.
- A buffer overflow, out-of-bounds read or integer overflow reachable from
  malformed input.
- A gap between what `docs/abi.md` guarantees and what the code does: an entry
  point that writes past the declared buffer, or lets a panic escape to the
  caller.
- Anything that would execute code from loaded content — **nothing the library
  loads allows it, and that is an invariant**: a texture is a block of pixels,
  and the formats to come will hold only identifiers, positions and dimensions.
  No binary, no script, no file path.

## Out of scope

**A host that violates the ABI's preconditions.** Passing an invalid pointer, a
`stride` smaller than the width, an already-destroyed handle: those conditions
are documented, and honouring them is the caller's responsibility. That is the
nature of a C boundary, not a defect in the library.

Editing your own files to get a different image. There is nothing here to
protect against its owner.
