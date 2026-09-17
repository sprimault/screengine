# Screengine

Français : [README.fr.md](README.fr.md)

A software 3D rendering engine, callable from any language. No GPU, no window:
you hand it a scene and a buffer, it fills the buffer.

MIT or Apache-2.0, at your option — see [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE). Unless you state otherwise, any contribution
you submit for inclusion is dual licensed the same way, without additional terms
or conditions.

The target is the class of 1996–1998 software renderers, done properly: true
colour, perspective-correct texturing, mipmaps, lightmaps, fog, ordered-dither
texture filtering by default with bilinear as a quality level, low internal
resolution scaled up by an integer factor. That class ran in software on a 1998
PC; a current phone core is many times faster, and the headroom goes to battery
and heat.

Everything after projection is fixed-point, and rendering is tile-based: the
same scene gives the same image to the bit on every target, every SIMD path,
every tile size and every thread count.

## What it does not do

That comes first, because it is what makes the engine embeddable. It opens no
window, reads no keyboard, opens no file, plays no sound, and knows nothing
about games — no player, no weapon, no score.

The host provides the window, the input and the bytes. The engine transforms and
renders. That boundary is the only reason the same code runs on Windows, in a
browser and on a phone.

## Status

**Step 0 in progress: a hardcoded triangle, and no engine.** The C boundary is
written, and the triangle is filled by the final fixed-point edge functions. It
remains to display it from four hosts — C, C++, wasm, Android — before any
other line of engine.

The roadmap has ten steps, each one published.

- [`ROADMAP.md`](ROADMAP.md) — the steps, which are cleared, and what is out of
  scope for v1 (French)
- [`CHANGELOG.md`](CHANGELOG.md) — what each version brought, dated
- [`docs/abi.md`](docs/abi.md) — the C boundary contract, which is authoritative
  (French; the generated header carries the essentials in English)
- [`docs/construction.md`](docs/construction.md) — targets, build matrix, header
  generation (French)

## Usage

Two paths, depending on what you are writing.

### Making a game, in Rust

`screengine-play` provides the window, keyboard, mouse and a fixed-step loop,
and scales the image up by an integer factor. With no settings at all, it opens
a working window:

```rust
use screengine_play::{KeyCode, Play};

fn main() -> Result<(), screengine_play::Error> {
    Play::new().run(
        (),
        |_, tick| {
            if tick.input().pressed(KeyCode::Space) {
                println!("step {}", tick.index());
            }
        },
        |_, _context| {},
    )
}
```

`make run` launches this example. The Rust path adds convenience, never
capability: everything it allows can also be done through the C ABI.

### Embedding, from any language

The host keeps its window, loop and input. The library exposes a stable C ABI
prefixed `scg_`, whose header is generated:

```c
ScgContextConfig config = {0};
config.max_width = config.width  = 640;
config.max_height = config.height = 360;
config.tile_size = 64;

ScgContext *ctx;
if (scg_create(&config, &ctx) != SCG_OK) {
    fprintf(stderr, "%s\n", scg_last_error(NULL));
    return 1;
}

scg_frame_end(ctx, pixels, stride);
scg_destroy(ctx);
```

Opaque handles, no callbacks, no allocation crossing the boundary. Every
fallible function returns a code, and whatever it produces goes through an out
parameter. Bindings live in separate repositories and hold nothing but type
conversion.

On the web the host cannot pass an arbitrary pointer: it allocates its buffer
through `scg_buffer_alloc` and builds a view over it. That is the only
difference between platforms.

## Building

```
make build     # core and shared library
make run       # opens a window on the engine
make header    # regenerates include/screengine.h
make test      # including the C and C++ hosts, if a compiler is present
make conform   # replays the reference scenes and compares hashes
make lint
make nostd     # proof that the core builds without std
```

The core is `no_std` and has no dependencies. A fresh clone builds with nothing
installed beyond a Rust toolchain; only `screengine-play` carries dependencies,
`winit` and `softbuffer`.

The Android and wasm targets each need their own tooling and go through CI. iOS
waits until the rest is stable — see the roadmap.
[`docs/construction.md`](docs/construction.md) carries the full matrix (in
French).
