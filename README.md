# Screengine

Français : [README.fr.md](README.fr.md)

A software 3D rendering engine, callable from any language. No GPU, no window:
you hand it a scene and a buffer, it fills the buffer.

MIT — see [`LICENSE`](LICENSE).

The look is that of late-nineties games, and that is deliberate: 256-colour
indexed buffer, table-driven distance falloff, affine texturing corrected per
segment, no filtering, low internal resolution scaled up by an integer factor.
Those traits cannot be bolted on afterwards — they belong to the pipeline.

## What it does not do

That comes first, because it is what makes the engine embeddable. It opens no
window, reads no keyboard, opens no file, plays no sound, and knows nothing
about games — no player, no weapon, no score.

The host provides the window, the input and the bytes. The engine transforms and
renders. That boundary is the only reason the same code runs on Windows, in a
browser and on a phone.

## Status

**Nothing is written yet.** The project starts at step 0: display a triangle
from four hosts — C, PHP, wasm, Android — before a single line of engine. Until
that step is cleared, the library panics on `todo!`.

The roadmap has ten steps, each one published.

- [`ROADMAP.md`](ROADMAP.md) — the steps, which are cleared, and what is out of
  scope for v1 (French)
- [`CHANGELOG.md`](CHANGELOG.md) — what each version brought, dated
- [`docs/abi.md`](docs/abi.md) — the C boundary contract, which is authoritative
  (French; the generated header carries the essentials in English)
- [`docs/construction.md`](docs/construction.md) — targets, build matrix, header
  generation (French)

## Usage

From Rust the API is idiomatic and the crate links directly. From everything
else, the library exposes a stable C ABI prefixed `scg_`, whose header is
generated:

```c
ScgContext* ctx = scg_create(640, 360);
scg_camera_set(ctx, pos, yaw, pitch, fov);
scg_frame_begin(ctx);
scg_draw_mesh(ctx, mesh, matrix);
scg_frame_end(ctx, pixels, stride);
```

Seventeen functions, opaque handles, no callbacks, no allocation crossing the
boundary. Bindings live in separate repositories and hold nothing but type
conversion.

On the web the host cannot pass an arbitrary pointer: it allocates its buffer
through `scg_buffer_alloc` and builds a view over it. That is the only
difference between platforms.

## Building

```
make build     # core and shared library
make header    # regenerates include/screengine.h
make test
make conform   # replays the reference scenes and compares hashes
make lint
make nostd     # proof that the core builds without std
```

The core is `no_std` and has no dependencies. A fresh clone builds with nothing
installed beyond a Rust toolchain.

The Android and wasm targets each need their own tooling and go through CI. iOS
waits until the rest is stable — see the roadmap.
[`docs/construction.md`](docs/construction.md) carries the full matrix (in
French).
