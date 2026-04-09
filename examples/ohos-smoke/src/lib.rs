use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dpi::PhysicalSize;
use pollster::block_on;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use wgpu::{
    Backends, Color, CommandEncoderDescriptor, CompositeAlphaMode, Device, DeviceDescriptor,
    Features, FragmentState, Instance, InstanceDescriptor, Limits, LoadOp, MemoryHints,
    MultisampleState, Operations, PipelineCompilationOptions, PipelineLayoutDescriptor,
    PowerPreference, PresentMode, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, RequestAdapterOptions,
    ShaderModuleDescriptor, ShaderSource, StoreOp, Surface, SurfaceConfiguration, SurfaceError,
    TextureUsages, TextureViewDescriptor, VertexState,
};
use winit_core::application::ApplicationHandler;
use winit_core::event::{StartCause, WindowEvent};
use winit_core::event_loop::ActiveEventLoop as CoreActiveEventLoop;
use winit_core::event_loop::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit_core::window::{Window as CoreWindow, WindowAttributes};
use tgui_winit_ohos::{
    EventLoop, OhosApp, OhosKeyAction, OhosMouseAction, OhosMouseButton, OhosPointerSource,
    OhosTouchAction, Window as OhosWindow,
};

const TRIANGLE_SHADER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.72),
        vec2<f32>(-0.72, -0.52),
        vec2<f32>(0.72, -0.52),
    );

    let xy = positions[vertex_index];
    return vec4<f32>(xy, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.98, 0.40, 0.19, 1.0);
}
"#;

pub struct SmokeRuntime {
    app: OhosApp,
    shared: Arc<SmokeShared>,
    stop_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct SmokeShared {
    message: Mutex<String>,
    sticky_failure: Mutex<Option<String>>,
    event_count: AtomicU64,
    redraw_count: AtomicU64,
}

impl SmokeShared {
    fn set_message(&self, message: impl Into<String>) {
        *self.message.lock().unwrap() = message.into();
    }

    fn snapshot_message(&self) -> Vec<u8> {
        let sticky = self.sticky_failure.lock().unwrap().clone();
        sticky
            .unwrap_or_else(|| self.message.lock().unwrap().clone())
            .into_bytes()
    }

    fn set_failure(&self, message: impl Into<String>) {
        let message = message.into();
        *self.sticky_failure.lock().unwrap() = Some(message.clone());
        *self.message.lock().unwrap() = message;
    }

    fn has_failure(&self) -> bool {
        self.sticky_failure.lock().unwrap().is_some()
    }
}

struct SmokeApp {
    shared: Arc<SmokeShared>,
    stop_requested: Arc<AtomicBool>,
    window: Option<Box<dyn CoreWindow>>,
    renderer: Option<Renderer>,
}

struct Renderer {
    surface: Surface<'static>,
    instance: Instance,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    pipeline: RenderPipeline,
}

enum RenderStatus {
    Rendered,
    Skipped,
    OutOfMemory,
}

impl SmokeApp {
    fn new(shared: Arc<SmokeShared>, stop_requested: Arc<AtomicBool>) -> Self {
        shared.set_message("waiting for surface");
        Self {
            shared,
            stop_requested,
            window: None,
            renderer: None,
        }
    }

    fn remember(&self, message: impl Into<String>) {
        self.shared.event_count.fetch_add(1, Ordering::Relaxed);
        self.shared.set_message(message);
    }

    fn remember_failure(&self, message: impl Into<String>) {
        self.shared.event_count.fetch_add(1, Ordering::Relaxed);
        self.shared.set_failure(message);
    }

    fn ensure_window(
        &mut self,
        event_loop: &dyn CoreActiveEventLoop,
    ) -> Result<&dyn CoreWindow, String> {
        if self.window.is_none() {
            let window = event_loop
                .create_window(WindowAttributes::default().with_title("tgui-winit-ohos triangle"))
                .map_err(|err| format!("create_window failed: {err}"))?;
            self.window = Some(window);
        }

        Ok(self.window.as_deref().expect("window was just created"))
    }

    fn ensure_renderer(&mut self, event_loop: &dyn CoreActiveEventLoop) -> Result<(), String> {
        if self.renderer.is_some() {
            return Ok(());
        }

        let renderer = {
            let window = self.ensure_window(event_loop)?;
            let backend_window = window
                .cast_ref::<OhosWindow>()
                .ok_or_else(|| String::from("window is not an OHOS backend window"))?;
            let size = window.surface_size();
            block_on(Renderer::new(backend_window, size))?
        };

        self.shared.set_message(format!(
            "triangle renderer ready: {}x{}",
            renderer.config.width, renderer.config.height
        ));
        self.renderer = Some(renderer);
        self.request_redraw();
        Ok(())
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler for SmokeApp {
    fn new_events(&mut self, _event_loop: &dyn CoreActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init) {
            self.remember("booting renderer");
        }
    }

    fn resumed(&mut self, _event_loop: &dyn CoreActiveEventLoop) {
        self.remember("surface resumed");
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn CoreActiveEventLoop) {
        self.remember("creating renderer");
        if let Err(err) = self.ensure_renderer(event_loop) {
            self.remember_failure(format!("renderer init failed: {err}"));
            event_loop.exit();
        }
    }

    fn proxy_wake_up(&mut self, _event_loop: &dyn CoreActiveEventLoop) {
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &dyn CoreActiveEventLoop,
        _window_id: winit_core::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::SurfaceResized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = self.window.as_ref() {
                    let size = window.surface_size();
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.resize(size.width, size.height);
                    }
                }
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    match renderer.render() {
                        Ok(RenderStatus::Rendered) => {
                            self.shared.redraw_count.fetch_add(1, Ordering::Relaxed);
                            self.shared.set_message("triangle rendered");
                        }
                        Ok(RenderStatus::Skipped) => {
                            self.shared.set_message("frame skipped; retrying");
                            self.request_redraw();
                        }
                        Ok(RenderStatus::OutOfMemory) => {
                            self.remember_failure("wgpu surface ran out of memory");
                            event_loop.exit();
                        }
                        Err(err) => {
                            self.remember_failure(format!("render failed: {err}"));
                        }
                    }
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.remember("window closing");
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn CoreActiveEventLoop) {
        if self.stop_requested.load(Ordering::Acquire) {
            self.remember("stop requested");
            event_loop.exit();
        }
    }

    fn suspended(&mut self, _event_loop: &dyn CoreActiveEventLoop) {
        self.remember("surface suspended");
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn CoreActiveEventLoop) {
        self.renderer = None;
        self.remember("renderer released");
    }

    fn memory_warning(&mut self, _event_loop: &dyn CoreActiveEventLoop) {
        self.remember("memory warning");
    }
}

impl Renderer {
    async fn new(window: &OhosWindow, size: PhysicalSize<u32>) -> Result<Self, String> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let raw_display_handle = window
            .display_handle()
            .map_err(|err| format!("display handle unavailable: {err}"))?
            .as_raw();
        let raw_window_handle = window
            .window_handle()
            .map_err(|err| format!("window handle unavailable: {err}"))?
            .as_raw();

        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|err| format!("create_surface failed: {err}"))?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|err| format!("request_adapter failed: {err}"))?;
        let adapter_info = adapter.get_info();
        let required_limits = adapter.limits();

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("tgui-winit-ohos device"),
                required_features: Features::empty(),
                required_limits,
                memory_hints: MemoryHints::Performance,
                ..Default::default()
            })
            .await
            .map_err(|err| format!("request_device failed: {err}"))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| String::from("surface reported no supported formats"))?;
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| matches!(mode, CompositeAlphaMode::Opaque))
            .or_else(|| capabilities.alpha_modes.first().copied())
            .ok_or_else(|| String::from("surface reported no alpha modes"))?;

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("triangle shader"),
            source: ShaderSource::Wgsl(TRIANGLE_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("triangle pipeline layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("triangle pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let renderer = Self {
            surface,
            instance,
            device,
            queue,
            config,
            pipeline,
        };
        eprintln!(
            "wgpu renderer ready: backend={:?}, device={}, format={:?}, size={}x{}",
            adapter_info.backend,
            adapter_info.name,
            renderer.config.format,
            renderer.config.width,
            renderer.config.height
        );
        Ok(renderer)
    }

    fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self) -> Result<RenderStatus, String> {
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(RenderStatus::Skipped);
        }

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Timeout) => return Ok(RenderStatus::Skipped),
            Err(SurfaceError::Outdated | SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(RenderStatus::Skipped);
            }
            Err(SurfaceError::OutOfMemory) => return Ok(RenderStatus::OutOfMemory),
            Err(err) => return Err(format!("surface frame acquisition failed: {err}")),
        };

        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("triangle encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("triangle pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color {
                            r: 0.02,
                            g: 0.36,
                            b: 0.48,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
        let _ = &self.instance;
        Ok(RenderStatus::Rendered)
    }
}

fn runtime_new() -> SmokeRuntime {
    let event_loop = EventLoop::bootstrap().expect("default OHOS event loop bootstrap failed");
    let app = event_loop.ohos_app().clone();
    let shared = Arc::new(SmokeShared::default());
    let stop_requested = Arc::new(AtomicBool::new(false));

    let worker_shared = shared.clone();
    let worker_stop = stop_requested.clone();
    let worker = thread::spawn(move || {
        let mut event_loop = event_loop;

        let mut demo = SmokeApp::new(worker_shared.clone(), worker_stop.clone());
        loop {
            match event_loop.pump_app_events(Some(Duration::from_millis(16)), &mut demo) {
                PumpStatus::Continue if worker_stop.load(Ordering::Acquire) => break,
                PumpStatus::Continue => {}
                PumpStatus::Exit(code) => {
                    if !worker_shared.has_failure() {
                        worker_shared.set_message(format!("event loop exited with code {code}"));
                    }
                    break;
                }
            }
        }
    });

    SmokeRuntime {
        app,
        shared,
        stop_requested,
        worker: Some(worker),
    }
}

unsafe fn runtime_ref<'a>(runtime: *const SmokeRuntime) -> Option<&'a SmokeRuntime> {
    unsafe { runtime.as_ref() }
}

unsafe fn runtime_mut<'a>(runtime: *mut SmokeRuntime) -> Option<&'a mut SmokeRuntime> {
    unsafe { runtime.as_mut() }
}

#[unsafe(no_mangle)]
pub extern "C" fn ohos_smoke_runtime_new() -> *mut SmokeRuntime {
    Box::into_raw(Box::new(runtime_new()))
}

/// # Safety
///
/// `runtime` must be a pointer returned by `ohos_smoke_runtime_new` and not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_free(runtime: *mut SmokeRuntime) {
    if let Some(runtime) = unsafe { runtime_mut(runtime) } {
        runtime.stop_requested.store(true, Ordering::Release);
        runtime.app.notify_frame();
        if let Some(worker) = runtime.worker.take() {
            let _ = worker.join();
        }
    }

    if !runtime.is_null() {
        unsafe {
            drop(Box::from_raw(runtime));
        }
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer. `buffer` may be null when `capacity` is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_copy_message(
    runtime: *const SmokeRuntime,
    buffer: *mut u8,
    capacity: usize,
) -> usize {
    let Some(runtime) = (unsafe { runtime_ref(runtime) }) else {
        return 0;
    };
    let bytes = runtime.shared.snapshot_message();
    let required = bytes.len();
    if capacity == 0 || buffer.is_null() {
        return required;
    }

    let copy_len = required.min(capacity.saturating_sub(1));
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, copy_len);
        *buffer.add(copy_len) = 0;
    }
    required
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_event_count(runtime: *const SmokeRuntime) -> u64 {
    unsafe { runtime_ref(runtime) }
        .map(|runtime| runtime.shared.event_count.load(Ordering::Relaxed))
        .unwrap_or(0)
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_redraw_count(runtime: *const SmokeRuntime) -> u64 {
    unsafe { runtime_ref(runtime) }
        .map(|runtime| runtime.shared.redraw_count.load(Ordering::Relaxed))
        .unwrap_or(0)
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_surface_created(
    runtime: *const SmokeRuntime,
    xcomponent: *mut c_void,
    native_window: *mut c_void,
    width: u32,
    height: u32,
    scale_factor: f64,
) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime
            .app
            .notify_surface_created(xcomponent, native_window, width, height, scale_factor);
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_surface_changed(
    runtime: *const SmokeRuntime,
    xcomponent: *mut c_void,
    native_window: *mut c_void,
    width: u32,
    height: u32,
    scale_factor: f64,
) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime
            .app
            .notify_surface_changed(xcomponent, native_window, width, height, scale_factor);
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_surface_destroyed(runtime: *const SmokeRuntime) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_surface_destroyed();
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_focus(runtime: *const SmokeRuntime, focused: bool) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_focus(focused);
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_visibility(
    runtime: *const SmokeRuntime,
    visible: bool,
) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_visibility(visible);
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_low_memory(runtime: *const SmokeRuntime) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_low_memory();
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_frame(runtime: *const SmokeRuntime) {
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_frame();
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_key(
    runtime: *const SmokeRuntime,
    action: u32,
    key_code: u32,
    repeat: bool,
    device_id: i64,
) {
    let Some(action) = OhosKeyAction::from_raw(action) else {
        return;
    };
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_key(action, key_code, repeat, device_id);
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_touch(
    runtime: *const SmokeRuntime,
    action: u32,
    source: u32,
    finger_id: u64,
    x: f64,
    y: f64,
    force: f64,
    has_force: bool,
    device_id: i64,
    primary: bool,
) {
    let Some(action) = OhosTouchAction::from_raw(action) else {
        return;
    };
    let Some(source) = OhosPointerSource::from_raw(source) else {
        return;
    };
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime.app.notify_touch(
            action,
            source,
            finger_id,
            x,
            y,
            has_force.then_some(force),
            device_id,
            primary,
        );
    }
}

/// # Safety
///
/// `runtime` must be a valid runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ohos_smoke_runtime_mouse(
    runtime: *const SmokeRuntime,
    action: u32,
    button: u32,
    has_button: bool,
    x: f64,
    y: f64,
    delta_x: f64,
    delta_y: f64,
    device_id: i64,
    primary: bool,
) {
    let Some(action) = OhosMouseAction::from_raw(action) else {
        return;
    };
    let button = if has_button {
        OhosMouseButton::from_raw(button)
    } else {
        None
    };
    if let Some(runtime) = unsafe { runtime_ref(runtime) } {
        runtime
            .app
            .notify_mouse(action, button, x, y, delta_x, delta_y, device_id, primary);
    }
}
