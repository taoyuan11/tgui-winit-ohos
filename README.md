# tgui-winit-ohos

OpenHarmony backend crate for the split `winit` architecture.

This backend targets `winit-core = 0.31.0-beta.2` and uses ArkUI `NativeXComponent`
as the host surface model.

## Installation

```toml
[dependencies]
tgui-winit-ohos = "0.0.1"
winit-core = "0.31.0-beta.2"
```

Rust code should import the crate as `tgui_winit_ohos`.

## Status

`tgui-winit-ohos` is currently a single-window backend aimed at `wgpu` and other rendering
libraries that only need a stable `raw-window-handle`, lifecycle callbacks, redraw
requests, and pointer/keyboard input.

Supported well:

- Single window creation
- Surface create/change/destroy lifecycle
- `raw-window-handle` for OHOS display and native window handles
- OHOS-specific density scale access through `WindowExtOhos::density_scale()`
- OHOS-specific font scale access through `WindowExtOhos::font_scale()`
- Pointer, mouse, wheel, focus, visibility, and keyboard input
- Redraw requests gated by host frame callbacks

Still intentionally unsupported in this release:

- Multiple windows
- Custom cursors and cursor grab/hittest
- Window dragging, positioning, and fullscreen management
- Deep IME integration beyond enable/disable state tracking

## Recommended Rust Entry Point

The simplest way to bootstrap the backend is to let `EventLoop` create a default `OhosApp`
for you, then clone that handle into the host shell that forwards OHOS callbacks:

```rust
use tgui_winit_ohos::EventLoop;

let mut event_loop = EventLoop::bootstrap()?;
let ohos_app = event_loop.ohos_app().clone();
```

If you need full control over host wiring, the advanced path is still available:

```rust
use tgui_winit_ohos::{EventLoop, EventLoopBuilderExtOhos, OhosApp};

let app = OhosApp::new();
let mut builder = EventLoop::builder();
builder.with_ohos_app(app.clone());
let mut event_loop = builder.build()?;
```

## Host Integration Notes

The OHOS shell should continue to forward `NativeXComponent` callbacks into `OhosApp`:

- `notify_surface_created`
- `notify_surface_changed`
- `notify_surface_destroyed`
- `notify_focus`
- `notify_visibility`
- `notify_low_memory`
- `notify_frame`
- `notify_touch`
- `notify_mouse`
- `notify_key`

When forwarding surface lifecycle, also include the current screen density scale and system font
scale so Rust code can query them later through `WindowExtOhos::density_scale()` and
`WindowExtOhos::font_scale()`.

`Window::request_redraw()` now records a redraw request and waits for a host frame callback
before emitting `WindowEvent::RedrawRequested`, which keeps redraw pacing stable.

## Automatic Shell Integration

If you package your app with `cargo-ohos-app`, you can now use a generated OHOS shell instead
of writing the callback bridge by hand.

In your Rust crate, export the standard runtime bridge once:

```rust
use winit_core::application::ApplicationHandler;
use tgui_winit_ohos::export_ohos_winit_app;

#[derive(Default)]
struct MyApp;

impl ApplicationHandler for MyApp {}

export_ohos_winit_app!(MyApp::default);
```

When `cargo-ohos-app` detects a dependency on `tgui-winit-ohos`, it can generate an
`XComponent + NativeXComponent` shell that forwards lifecycle and input callbacks into these
exports automatically.

The crate's DevEco log helper is aligned with that shell now and writes through
`cargo-ohos-app`'s `cargo_ohos_app_hilog(level, domain, tag, message)` bridge instead of
calling `hilog_ndk.z` directly. You can keep using the high-level helper:

```rust
use tgui_winit_ohos::{OhosLogLevel, deveco_log, deveco_log_with_level};

deveco_log("surface created");
deveco_log_with_level(OhosLogLevel::Warn, "frame skipped");
```

The recommended end-to-end packaging example now lives in the companion
`harmony-app/examples/winit-smoke` project, where `cargo-ohos-app` packages the app as an
`x86_64-unknown-linux-ohos` simulator `.hap` by default:

```powershell
cd <path-to-harmony-app>
cargo run -- package --manifest-path .\examples\winit-smoke\Cargo.toml
```

`examples/ohos-smoke` remains in this repository as a lower-level smoke/reference example for the
renderer and runtime wiring, but the shared packaging workflow is centered on the
`harmony-app/examples/winit-smoke` crate.
