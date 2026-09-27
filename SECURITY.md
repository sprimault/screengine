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

**That means texture pixel blocks, mesh and map files, and lightmap cache
blocks.** The last three landed with steps 4 and 5, and this section now names
them: a scope that claims less than the code does discourages a useful report,
just as one that claims more wastes the reporter's time.

Everything inside the bytes handed over is treated as hostile, without
exception. That the pointer-length pair really covers the bytes it announces
remains a caller precondition, however: the engine cannot check it.

- A texture description or pixel block that crashes the loader, reads out of
  bounds, or overflows an integer on its way to an allocation size.
- A mesh, map or lightmap cache file achieving the same — a count that does not
  square with its section length, an out-of-range index, an overlapping section
  table, an allocation size taken from a declared number.
- A buffer overflow, out-of-bounds read or integer overflow reachable from
  malformed input.
- A gap between what `docs/abi.md` guarantees and what the code does: an entry
  point that writes past the declared buffer, or lets a panic escape to the
  caller.
- Anything that would execute code from loaded content — **nothing the library
  loads allows it, and that is an invariant**: a texture is a block of pixels,
  and the formats hold only identifiers, positions, dimensions, and bytes the
  engine copies without ever reading them. No binary, no script, no file path.

## Out of scope

**A host that violates the ABI's preconditions.** Passing an invalid pointer, a
`stride` smaller than the width, an already-destroyed handle: those conditions
are documented, and honouring them is the caller's responsibility. That is the
nature of a C boundary, not a defect in the library.

Editing your own files to get a different image. There is nothing here to
protect against its owner.
