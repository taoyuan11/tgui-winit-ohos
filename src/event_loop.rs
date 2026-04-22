use std::collections::HashSet;
use std::ffi::c_void;
use std::fmt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dpi::{PhysicalInsets, PhysicalPosition, PhysicalSize, Position, Size};
use raw_window_handle as rwh_06;
use smol_str::SmolStr;
use tracing::warn;
use winit_core::application::ApplicationHandler;
use winit_core::cursor::{Cursor, CustomCursor, CustomCursorSource};
use winit_core::error::{EventLoopError, NotSupportedError, RequestError};
use winit_core::event::{
    self, ButtonSource, DeviceId, FingerId, Force, KeyEvent, Modifiers, MouseButton,
    MouseScrollDelta, PointerKind, PointerSource, StartCause, SurfaceSizeWriter, TouchPhase,
    WindowEvent,
};
use winit_core::event_loop::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit_core::event_loop::run_on_demand::EventLoopExtRunOnDemand;
use winit_core::event_loop::{
    ActiveEventLoop as RootActiveEventLoop, ControlFlow, DeviceEvents,
    EventLoopProxy as CoreEventLoopProxy, EventLoopProxyProvider,
    OwnedDisplayHandle as CoreOwnedDisplayHandle,
};
use winit_core::keyboard::{ModifiersKeys, ModifiersState};
use winit_core::monitor::{Fullscreen, MonitorHandle as CoreMonitorHandle};
use winit_core::window::{
    self, CursorGrabMode, ImeCapabilities, ImePurpose, ImeRequest, ImeRequestError,
    ResizeDirection, Theme, Window as CoreWindow, WindowAttributes, WindowButtons, WindowId,
    WindowLevel,
};

use crate::app::{
    HostEvent, MouseEvent, OhosApp, OhosKeyAction, OhosMouseAction, OhosMouseButton,
    OhosPointerSource, OhosTouchAction, SurfaceEvent, TouchEvent,
};
use crate::keycodes;

static EVENT_LOOP_CREATED: AtomicBool = AtomicBool::new(false);
const GLOBAL_WINDOW: WindowId = WindowId::from_raw(0);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct PlatformSpecificEventLoopAttributes {
    pub ohos_app: Option<OhosApp>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformSpecificWindowAttributes;

impl winit_core::window::PlatformWindowAttributes for PlatformSpecificWindowAttributes {
    fn box_clone(&self) -> Box<dyn winit_core::window::PlatformWindowAttributes> {
        Box::new(*self)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventLoopBuilder {
    attributes: PlatformSpecificEventLoopAttributes,
}

pub trait EventLoopBuilderExtOhos {
    fn with_ohos_app(&mut self, app: OhosApp) -> &mut Self;
    fn with_default_ohos_app(&mut self) -> &mut Self;
}

impl EventLoopBuilderExtOhos for EventLoopBuilder {
    fn with_ohos_app(&mut self, app: OhosApp) -> &mut Self {
        self.attributes.ohos_app = Some(app);
        self
    }

    fn with_default_ohos_app(&mut self) -> &mut Self {
        self.attributes.ohos_app = Some(OhosApp::default());
        self
    }
}

impl EventLoopBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build(&self) -> Result<EventLoop, EventLoopError> {
        EventLoop::new(&self.attributes)
    }
}

#[derive(Debug)]
pub struct EventLoop {
    ohos_app: OhosApp,
    pub(crate) window_target: ActiveEventLoop,
    loop_running: bool,
}

struct ProxyProvider {
    ohos_app: OhosApp,
    wake_up: AtomicBool,
}

impl fmt::Debug for ProxyProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyProvider").finish_non_exhaustive()
    }
}

impl ProxyProvider {
    fn new(ohos_app: OhosApp) -> Self {
        Self {
            ohos_app,
            wake_up: AtomicBool::new(false),
        }
    }

    fn take_wake_up(&self) -> bool {
        self.wake_up.swap(false, Ordering::AcqRel)
    }
}

impl EventLoopProxyProvider for ProxyProvider {
    fn wake_up(&self) {
        self.wake_up.store(true, Ordering::Release);
        self.ohos_app.wake_proxy();
    }
}

#[derive(Debug)]
pub struct ActiveEventLoop {
    pub(crate) ohos_app: OhosApp,
    control_flow: Mutex<ControlFlow>,
    exit: AtomicBool,
    shared: Arc<SharedWindowState>,
    event_loop_proxy: Arc<ProxyProvider>,
}

#[derive(Debug)]
struct SharedWindowState {
    inner: Mutex<WindowInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecycleState {
    Uninitialized,
    SurfaceReady,
    Running,
    Suspended,
    Destroyed,
}

#[derive(Debug)]
struct WindowInner {
    lifecycle: LifecycleState,
    xcomponent: Option<usize>,
    native_window: Option<usize>,
    surface_size: PhysicalSize<u32>,
    scale_factor: f64,
    font_scale: f64,
    focused: bool,
    occluded: bool,
    window_created: bool,
    redraw_requested: bool,
    frame_ready: bool,
    mouse_inside: bool,
    modifiers: ModifiersState,
    modifier_keys: ModifiersKeys,
    pressed_fingers: HashSet<u64>,
    ime_capabilities: Option<ImeCapabilities>,
}

impl WindowInner {
    fn surface_available(&self) -> bool {
        matches!(
            self.lifecycle,
            LifecycleState::SurfaceReady | LifecycleState::Running
        )
    }
}

impl SharedWindowState {
    fn new() -> Self {
        Self {
            inner: Mutex::new(WindowInner {
                lifecycle: LifecycleState::Uninitialized,
                xcomponent: None,
                native_window: None,
                surface_size: PhysicalSize::new(0, 0),
                scale_factor: 1.0,
                font_scale: 1.0,
                focused: false,
                occluded: false,
                window_created: false,
                redraw_requested: false,
                frame_ready: false,
                mouse_inside: false,
                modifiers: ModifiersState::empty(),
                modifier_keys: ModifiersKeys::empty(),
                pressed_fingers: HashSet::new(),
                ime_capabilities: None,
            }),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct OwnedDisplayHandle;

impl fmt::Debug for OwnedDisplayHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedDisplayHandle").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Window {
    shared: Arc<SharedWindowState>,
    ohos_app: OhosApp,
}

impl EventLoop {
    pub fn builder() -> EventLoopBuilder {
        EventLoopBuilder::new()
    }

    pub fn bootstrap() -> Result<Self, EventLoopError> {
        Self::builder().build()
    }

    pub fn new(attributes: &PlatformSpecificEventLoopAttributes) -> Result<Self, EventLoopError> {
        if EVENT_LOOP_CREATED.swap(true, Ordering::AcqRel) {
            return Err(EventLoopError::RecreationAttempt);
        }

        let ohos_app = attributes.ohos_app.clone().unwrap_or_default();
        let shared = Arc::new(SharedWindowState::new());
        let event_loop_proxy = Arc::new(ProxyProvider::new(ohos_app.clone()));

        Ok(Self {
            ohos_app: ohos_app.clone(),
            window_target: ActiveEventLoop {
                ohos_app,
                control_flow: Mutex::new(ControlFlow::Wait),
                exit: AtomicBool::new(false),
                shared,
                event_loop_proxy,
            },
            loop_running: false,
        })
    }

    pub fn ohos_app(&self) -> &OhosApp {
        &self.ohos_app
    }

    pub fn window_target(&self) -> &dyn RootActiveEventLoop {
        &self.window_target
    }

    pub fn create_proxy(&self) -> CoreEventLoopProxy {
        self.window_target.create_proxy()
    }

    fn control_flow(&self) -> ControlFlow {
        *self.window_target.control_flow.lock().unwrap()
    }

    fn exiting(&self) -> bool {
        self.window_target.exit.load(Ordering::Acquire)
    }

    fn single_iteration<A: ApplicationHandler>(
        &mut self,
        events: Vec<HostEvent>,
        cause: StartCause,
        app: &mut A,
    ) {
        app.new_events(self.window_target(), cause);

        for event in events {
            self.handle_host_event(event, app);
        }

        if self.window_target.event_loop_proxy.take_wake_up() {
            app.proxy_wake_up(self.window_target());
        }

        if self.take_redraw_ready() {
            app.window_event(
                self.window_target(),
                GLOBAL_WINDOW,
                WindowEvent::RedrawRequested,
            );
        }

        app.about_to_wait(self.window_target());
    }

    fn take_redraw_ready(&self) -> bool {
        let mut state = self.window_target.shared.inner.lock().unwrap();
        let should_emit = matches!(state.lifecycle, LifecycleState::Running)
            && state.redraw_requested
            && state.frame_ready;
        if should_emit {
            state.redraw_requested = false;
            state.frame_ready = false;
        }
        should_emit
    }

    fn handle_host_event<A: ApplicationHandler>(&mut self, event: HostEvent, app: &mut A) {
        match event {
            HostEvent::SurfaceCreated(event) => self.on_surface_created(event, app),
            HostEvent::SurfaceChanged(event) => self.on_surface_changed(event, app),
            HostEvent::SurfaceDestroyed => self.on_surface_destroyed(app),
            HostEvent::Focused(focused) => {
                self.window_target.shared.inner.lock().unwrap().focused = focused;
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::Focused(focused),
                );
            }
            HostEvent::Visible(visible) => {
                self.window_target.shared.inner.lock().unwrap().occluded = !visible;
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::Occluded(!visible),
                );
            }
            HostEvent::LowMemory => app.memory_warning(self.window_target()),
            HostEvent::FrameAvailable => {
                self.window_target.shared.inner.lock().unwrap().frame_ready = true
            }
            HostEvent::RedrawRequested => {
                self.window_target
                    .shared
                    .inner
                    .lock()
                    .unwrap()
                    .redraw_requested = true;
            }
            HostEvent::ProxyWake => {}
            HostEvent::Touch(event) => self.dispatch_touch_event(event, app),
            HostEvent::Mouse(event) => self.dispatch_mouse_event(event, app),
            HostEvent::Key(event) => self.dispatch_key_event(event, app),
        }
    }

    fn on_surface_created<A: ApplicationHandler>(&mut self, event: SurfaceEvent, app: &mut A) {
        let (previous_size, previous_scale, should_resume) = {
            let mut state = self.window_target.shared.inner.lock().unwrap();
            let previous_size = state.surface_size;
            let previous_scale = state.scale_factor;
            let should_resume = !state.surface_available();
            update_surface_state(&mut state, event);
            state.lifecycle = LifecycleState::SurfaceReady;
            state.redraw_requested = true;
            state.frame_ready = true;
            (previous_size, previous_scale, should_resume)
        };

        if should_resume {
            app.resumed(self.window_target());
            app.can_create_surfaces(self.window_target());
        }
        self.window_target.shared.inner.lock().unwrap().lifecycle = LifecycleState::Running;
        self.emit_surface_change_events(event, previous_size, previous_scale, app);
    }

    fn on_surface_changed<A: ApplicationHandler>(&mut self, event: SurfaceEvent, app: &mut A) {
        let (previous_size, previous_scale, should_resume) = {
            let mut state = self.window_target.shared.inner.lock().unwrap();
            let previous_size = state.surface_size;
            let previous_scale = state.scale_factor;
            let should_resume = !state.surface_available();
            update_surface_state(&mut state, event);
            state.lifecycle = LifecycleState::SurfaceReady;
            state.redraw_requested = true;
            state.frame_ready = true;
            (previous_size, previous_scale, should_resume)
        };

        if should_resume {
            app.resumed(self.window_target());
            app.can_create_surfaces(self.window_target());
        }

        self.window_target.shared.inner.lock().unwrap().lifecycle = LifecycleState::Running;
        self.emit_surface_change_events(event, previous_size, previous_scale, app);
    }

    fn on_surface_destroyed<A: ApplicationHandler>(&mut self, app: &mut A) {
        let should_suspend = {
            let mut state = self.window_target.shared.inner.lock().unwrap();
            if !state.surface_available() && matches!(state.lifecycle, LifecycleState::Destroyed) {
                return;
            }
            state.lifecycle = LifecycleState::Suspended;
            state.native_window = None;
            state.xcomponent = None;
            state.surface_size = PhysicalSize::new(0, 0);
            state.redraw_requested = false;
            state.frame_ready = false;
            state.mouse_inside = false;
            state.scale_factor = 1.0;
            state.font_scale = 1.0;
            state.pressed_fingers.clear();
            true
        };

        if should_suspend {
            app.destroy_surfaces(self.window_target());
            app.suspended(self.window_target());
            self.window_target.shared.inner.lock().unwrap().lifecycle = LifecycleState::Destroyed;
        }
    }

    fn emit_surface_change_events<A: ApplicationHandler>(
        &self,
        event: SurfaceEvent,
        previous_size: PhysicalSize<u32>,
        previous_scale: f64,
        app: &mut A,
    ) {
        let new_size = PhysicalSize::new(event.width, event.height);
        let new_scale = sanitize_scale_factor(event.scale_factor);

        if (new_scale - previous_scale).abs() > f64::EPSILON {
            let requested_size = Arc::new(Mutex::new(new_size));
            let writer = SurfaceSizeWriter::new(Arc::downgrade(&requested_size));
            app.window_event(
                self.window_target(),
                GLOBAL_WINDOW,
                WindowEvent::ScaleFactorChanged {
                    scale_factor: new_scale,
                    surface_size_writer: writer,
                },
            );

            let final_size = *requested_size.lock().unwrap();
            {
                let mut state = self.window_target.shared.inner.lock().unwrap();
                state.scale_factor = new_scale;
                state.surface_size = final_size;
            }
            app.window_event(
                self.window_target(),
                GLOBAL_WINDOW,
                WindowEvent::SurfaceResized(final_size),
            );
        } else if previous_size != new_size
            || matches!(
                previous_size,
                PhysicalSize {
                    width: 0,
                    height: 0
                }
            )
        {
            app.window_event(
                self.window_target(),
                GLOBAL_WINDOW,
                WindowEvent::SurfaceResized(new_size),
            );
        }
    }

    fn dispatch_touch_event<A: ApplicationHandler>(&mut self, event: TouchEvent, app: &mut A) {
        let finger_id = FingerId::from_raw(event.finger_id as usize);
        let device_id = Some(DeviceId::from_raw(event.device_id));
        let position = PhysicalPosition::new(event.x, event.y);
        let force = event.force.map(Force::Normalized);
        let kind = PointerKind::Touch(finger_id);
        let source = match event.source {
            OhosPointerSource::Touchscreen => PointerSource::Touch { finger_id, force },
            OhosPointerSource::Mouse => PointerSource::Mouse,
            OhosPointerSource::Touchpad | OhosPointerSource::Unknown => PointerSource::Unknown,
        };
        let button = ButtonSource::Touch { finger_id, force };

        match event.action {
            OhosTouchAction::Down => {
                let should_enter = self
                    .window_target
                    .shared
                    .inner
                    .lock()
                    .unwrap()
                    .pressed_fingers
                    .insert(event.finger_id);
                if should_enter {
                    app.window_event(
                        self.window_target(),
                        GLOBAL_WINDOW,
                        WindowEvent::PointerEntered {
                            device_id,
                            position,
                            primary: event.primary,
                            kind,
                        },
                    );
                }
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerButton {
                        device_id,
                        state: event::ElementState::Pressed,
                        position,
                        primary: event.primary,
                        button,
                    },
                );
            }
            OhosTouchAction::Move => {
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerMoved {
                        device_id,
                        position,
                        primary: event.primary,
                        source,
                    },
                );
            }
            OhosTouchAction::Up => {
                self.window_target
                    .shared
                    .inner
                    .lock()
                    .unwrap()
                    .pressed_fingers
                    .remove(&event.finger_id);
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerButton {
                        device_id,
                        state: event::ElementState::Released,
                        position,
                        primary: event.primary,
                        button,
                    },
                );
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerLeft {
                        device_id,
                        position: Some(position),
                        primary: event.primary,
                        kind,
                    },
                );
            }
            OhosTouchAction::Cancel => {
                self.window_target
                    .shared
                    .inner
                    .lock()
                    .unwrap()
                    .pressed_fingers
                    .remove(&event.finger_id);
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerLeft {
                        device_id,
                        position: Some(position),
                        primary: event.primary,
                        kind,
                    },
                );
            }
        }
    }

    fn dispatch_mouse_event<A: ApplicationHandler>(&mut self, event: MouseEvent, app: &mut A) {
        let device_id = Some(DeviceId::from_raw(event.device_id));
        let position = PhysicalPosition::new(event.x, event.y);

        match event.action {
            OhosMouseAction::Enter => {
                self.ensure_mouse_entered(device_id, position, event.primary, app);
            }
            OhosMouseAction::Leave | OhosMouseAction::Cancel => {
                let was_inside = {
                    let mut state = self.window_target.shared.inner.lock().unwrap();
                    let was_inside = state.mouse_inside;
                    state.mouse_inside = false;
                    was_inside
                };
                if was_inside {
                    app.window_event(
                        self.window_target(),
                        GLOBAL_WINDOW,
                        WindowEvent::PointerLeft {
                            device_id,
                            position: Some(position),
                            primary: event.primary,
                            kind: PointerKind::Mouse,
                        },
                    );
                }
            }
            OhosMouseAction::Move => {
                self.ensure_mouse_entered(device_id, position, event.primary, app);
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerMoved {
                        device_id,
                        position,
                        primary: event.primary,
                        source: PointerSource::Mouse,
                    },
                );
            }
            OhosMouseAction::ButtonDown | OhosMouseAction::ButtonUp => {
                self.ensure_mouse_entered(device_id, position, event.primary, app);
                let state = if matches!(event.action, OhosMouseAction::ButtonDown) {
                    event::ElementState::Pressed
                } else {
                    event::ElementState::Released
                };
                let button = event
                    .button
                    .map(map_mouse_button)
                    .map(ButtonSource::Mouse)
                    .unwrap_or(ButtonSource::Unknown(0));
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::PointerButton {
                        device_id,
                        state,
                        position,
                        primary: event.primary,
                        button,
                    },
                );
            }
            OhosMouseAction::Wheel => {
                self.ensure_mouse_entered(device_id, position, event.primary, app);
                app.window_event(
                    self.window_target(),
                    GLOBAL_WINDOW,
                    WindowEvent::MouseWheel {
                        device_id,
                        delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                            event.delta_x,
                            event.delta_y,
                        )),
                        phase: TouchPhase::Moved,
                    },
                );
            }
        }
    }

    fn ensure_mouse_entered<A: ApplicationHandler>(
        &self,
        device_id: Option<DeviceId>,
        position: PhysicalPosition<f64>,
        primary: bool,
        app: &mut A,
    ) {
        let should_enter = {
            let mut state = self.window_target.shared.inner.lock().unwrap();
            if state.mouse_inside {
                false
            } else {
                state.mouse_inside = true;
                true
            }
        };

        if should_enter {
            app.window_event(
                self.window_target(),
                GLOBAL_WINDOW,
                WindowEvent::PointerEntered {
                    device_id,
                    position,
                    primary,
                    kind: PointerKind::Mouse,
                },
            );
        }
    }

    fn dispatch_key_event<A: ApplicationHandler>(
        &mut self,
        event: crate::app::KeyEvent,
        app: &mut A,
    ) {
        let device_id = Some(DeviceId::from_raw(event.device_id));
        let state = match event.action {
            OhosKeyAction::Down => event::ElementState::Pressed,
            OhosKeyAction::Up => event::ElementState::Released,
            OhosKeyAction::Unknown => event::ElementState::Released,
        };

        let physical_key = keycodes::to_physical_key(event.key_code);
        let logical_key = keycodes::to_logical(event.key_code);
        let text = if state.is_pressed() {
            logical_key.to_text().map(SmolStr::new)
        } else {
            None
        };

        if let Some(modifiers) = update_modifiers(&self.window_target.shared, event.key_code, state)
        {
            app.window_event(
                self.window_target(),
                GLOBAL_WINDOW,
                WindowEvent::ModifiersChanged(modifiers),
            );
        }

        app.window_event(
            self.window_target(),
            GLOBAL_WINDOW,
            WindowEvent::KeyboardInput {
                device_id,
                event: KeyEvent {
                    physical_key,
                    logical_key: logical_key.clone(),
                    text: text.clone(),
                    location: keycodes::to_location(event.key_code),
                    state,
                    repeat: event.repeat,
                    text_with_all_modifiers: text,
                    key_without_modifiers: keycodes::to_logical(event.key_code),
                },
                is_synthetic: false,
            },
        );
    }
}

impl EventLoopExtRunOnDemand for EventLoop {
    fn run_app_on_demand<A: ApplicationHandler>(
        &mut self,
        mut app: A,
    ) -> Result<(), EventLoopError> {
        self.window_target.exit.store(false, Ordering::Release);
        loop {
            match self.pump_app_events(None, &mut app) {
                PumpStatus::Continue => continue,
                PumpStatus::Exit(0) => break Ok(()),
                PumpStatus::Exit(code) => break Err(EventLoopError::ExitFailure(code)),
            }
        }
    }
}

impl EventLoopExtPumpEvents for EventLoop {
    fn pump_app_events<A: ApplicationHandler>(
        &mut self,
        timeout: Option<Duration>,
        mut app: A,
    ) -> PumpStatus {
        if !self.loop_running {
            self.loop_running = true;
            self.single_iteration(Vec::new(), StartCause::Init, &mut app);
        }

        if self.exiting() {
            self.loop_running = false;
            return PumpStatus::Exit(0);
        }

        let start = Instant::now();
        let control_flow_timeout = match self.control_flow() {
            ControlFlow::Poll => Some(Duration::ZERO),
            ControlFlow::Wait => None,
            ControlFlow::WaitUntil(deadline) => Some(deadline.saturating_duration_since(start)),
        };
        let effective_timeout = min_timeout(timeout, control_flow_timeout);
        let events = self.ohos_app.wait_and_drain(effective_timeout);

        let cause = match self.control_flow() {
            ControlFlow::Poll => StartCause::Poll,
            ControlFlow::Wait => StartCause::WaitCancelled {
                start,
                requested_resume: None,
            },
            ControlFlow::WaitUntil(deadline) => {
                if Instant::now() >= deadline {
                    StartCause::ResumeTimeReached {
                        start,
                        requested_resume: deadline,
                    }
                } else {
                    StartCause::WaitCancelled {
                        start,
                        requested_resume: Some(deadline),
                    }
                }
            }
        };

        self.single_iteration(events, cause, &mut app);

        if self.exiting() {
            self.loop_running = false;
            PumpStatus::Exit(0)
        } else {
            PumpStatus::Continue
        }
    }
}

impl RootActiveEventLoop for ActiveEventLoop {
    fn create_proxy(&self) -> CoreEventLoopProxy {
        CoreEventLoopProxy::new(self.event_loop_proxy.clone())
    }

    fn create_window(
        &self,
        _window_attributes: WindowAttributes,
    ) -> Result<Box<dyn CoreWindow>, RequestError> {
        let mut state = self.shared.inner.lock().unwrap();
        if state.window_created {
            return Err(NotSupportedError::new("only a single OHOS window is supported").into());
        }
        state.window_created = true;
        drop(state);
        Ok(Box::new(Window {
            shared: self.shared.clone(),
            ohos_app: self.ohos_app.clone(),
        }))
    }

    fn create_custom_cursor(
        &self,
        _custom_cursor: CustomCursorSource,
    ) -> Result<CustomCursor, RequestError> {
        Err(NotSupportedError::new("custom cursors are not supported on OHOS").into())
    }

    fn available_monitors(&self) -> Box<dyn Iterator<Item = CoreMonitorHandle>> {
        Box::new(std::iter::empty())
    }

    fn primary_monitor(&self) -> Option<CoreMonitorHandle> {
        None
    }

    fn listen_device_events(&self, _allowed: DeviceEvents) {}

    fn system_theme(&self) -> Option<Theme> {
        None
    }

    fn set_control_flow(&self, control_flow: ControlFlow) {
        *self.control_flow.lock().unwrap() = control_flow;
    }

    fn control_flow(&self) -> ControlFlow {
        *self.control_flow.lock().unwrap()
    }

    fn exit(&self) {
        self.exit.store(true, Ordering::Release);
        self.ohos_app.wake_proxy();
    }

    fn exiting(&self) -> bool {
        self.exit.load(Ordering::Acquire)
    }

    fn owned_display_handle(&self) -> CoreOwnedDisplayHandle {
        CoreOwnedDisplayHandle::new(Arc::new(OwnedDisplayHandle))
    }

    fn rwh_06_handle(&self) -> &dyn rwh_06::HasDisplayHandle {
        self
    }
}

impl ActiveEventLoop {
    pub fn ohos_app(&self) -> &OhosApp {
        &self.ohos_app
    }
}

impl rwh_06::HasDisplayHandle for ActiveEventLoop {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        Ok(rwh_06::DisplayHandle::ohos())
    }
}

impl rwh_06::HasDisplayHandle for OwnedDisplayHandle {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        Ok(rwh_06::DisplayHandle::ohos())
    }
}

impl Window {
    pub fn xcomponent_ptr(&self) -> *mut c_void {
        self.shared
            .inner
            .lock()
            .unwrap()
            .xcomponent
            .map(|ptr| ptr as *mut c_void)
            .unwrap_or(std::ptr::null_mut())
    }

    pub fn native_window_ptr(&self) -> *mut c_void {
        self.shared
            .inner
            .lock()
            .unwrap()
            .native_window
            .map(|ptr| ptr as *mut c_void)
            .unwrap_or(std::ptr::null_mut())
    }

    pub fn font_scale(&self) -> f64 {
        self.shared.inner.lock().unwrap().font_scale
    }
}

impl rwh_06::HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        Ok(rwh_06::DisplayHandle::ohos())
    }
}

impl rwh_06::HasWindowHandle for Window {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let state = self.shared.inner.lock().unwrap();
        let native_window = if state.surface_available() {
            state.native_window
        } else {
            None
        }
        .ok_or(rwh_06::HandleError::Unavailable)?;
        let native_window =
            NonNull::new(native_window as *mut c_void).ok_or(rwh_06::HandleError::Unavailable)?;
        let raw = rwh_06::OhosNdkWindowHandle::new(native_window);
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw.into()) })
    }
}

impl CoreWindow for Window {
    fn id(&self) -> WindowId {
        GLOBAL_WINDOW
    }

    fn primary_monitor(&self) -> Option<CoreMonitorHandle> {
        None
    }

    fn available_monitors(&self) -> Box<dyn Iterator<Item = CoreMonitorHandle>> {
        Box::new(std::iter::empty())
    }

    fn current_monitor(&self) -> Option<CoreMonitorHandle> {
        None
    }

    fn scale_factor(&self) -> f64 {
        self.shared.inner.lock().unwrap().scale_factor
    }

    fn request_redraw(&self) {
        self.ohos_app.request_redraw();
    }

    fn pre_present_notify(&self) {}

    fn surface_position(&self) -> PhysicalPosition<i32> {
        (0, 0).into()
    }

    fn outer_position(&self) -> Result<PhysicalPosition<i32>, RequestError> {
        Err(NotSupportedError::new("window positioning is not supported on OHOS").into())
    }

    fn set_outer_position(&self, _position: Position) {}

    fn surface_size(&self) -> PhysicalSize<u32> {
        self.shared.inner.lock().unwrap().surface_size
    }

    fn request_surface_size(&self, _size: Size) -> Option<PhysicalSize<u32>> {
        Some(self.surface_size())
    }

    fn outer_size(&self) -> PhysicalSize<u32> {
        self.surface_size()
    }

    fn safe_area(&self) -> PhysicalInsets<u32> {
        PhysicalInsets::new(0, 0, 0, 0)
    }

    fn set_min_surface_size(&self, _size: Option<Size>) {}

    fn set_max_surface_size(&self, _size: Option<Size>) {}

    fn surface_resize_increments(&self) -> Option<PhysicalSize<u32>> {
        None
    }

    fn set_surface_resize_increments(&self, _increments: Option<Size>) {}

    fn set_title(&self, _title: &str) {}

    fn set_transparent(&self, _transparent: bool) {}

    fn set_blur(&self, _blur: bool) {}

    fn set_visible(&self, _visible: bool) {}

    fn is_visible(&self) -> Option<bool> {
        Some(!self.shared.inner.lock().unwrap().occluded)
    }

    fn set_resizable(&self, _resizable: bool) {}

    fn is_resizable(&self) -> bool {
        false
    }

    fn set_enabled_buttons(&self, _buttons: WindowButtons) {}

    fn enabled_buttons(&self) -> WindowButtons {
        WindowButtons::all()
    }

    fn set_minimized(&self, _minimized: bool) {}

    fn is_minimized(&self) -> Option<bool> {
        None
    }

    fn set_maximized(&self, _maximized: bool) {}

    fn is_maximized(&self) -> bool {
        false
    }

    fn set_fullscreen(&self, _fullscreen: Option<Fullscreen>) {
        warn!("fullscreen is not supported on OHOS");
    }

    fn fullscreen(&self) -> Option<Fullscreen> {
        None
    }

    fn set_decorations(&self, _decorations: bool) {}

    fn is_decorated(&self) -> bool {
        false
    }

    fn set_window_level(&self, _level: WindowLevel) {}

    fn set_window_icon(&self, _window_icon: Option<winit_core::icon::Icon>) {}

    fn set_ime_cursor_area(&self, _position: Position, _size: Size) {}

    fn request_ime_update(&self, request: ImeRequest) -> Result<(), ImeRequestError> {
        let mut state = self.shared.inner.lock().unwrap();
        match request {
            ImeRequest::Enable(enable) => {
                if state.ime_capabilities.is_some() {
                    return Err(ImeRequestError::AlreadyEnabled);
                }
                let (capabilities, _) = enable.into_raw();
                state.ime_capabilities = Some(capabilities);
                Ok(())
            }
            ImeRequest::Update(_) => {
                if state.ime_capabilities.is_none() {
                    Err(ImeRequestError::NotEnabled)
                } else {
                    Err(ImeRequestError::NotSupported)
                }
            }
            ImeRequest::Disable => {
                state.ime_capabilities = None;
                Ok(())
            }
        }
    }

    fn ime_capabilities(&self) -> Option<ImeCapabilities> {
        self.shared.inner.lock().unwrap().ime_capabilities
    }

    fn set_ime_purpose(&self, _purpose: ImePurpose) {}

    fn focus_window(&self) {}

    fn request_user_attention(&self, _request_type: Option<window::UserAttentionType>) {}

    fn set_cursor(&self, _cursor: Cursor) {}

    fn set_cursor_position(&self, _position: Position) -> Result<(), RequestError> {
        Err(NotSupportedError::new("cursor positioning is not supported on OHOS").into())
    }

    fn set_cursor_grab(&self, _mode: CursorGrabMode) -> Result<(), RequestError> {
        Err(NotSupportedError::new("cursor grab is not supported on OHOS").into())
    }

    fn set_cursor_visible(&self, _visible: bool) {}

    fn drag_window(&self) -> Result<(), RequestError> {
        Err(NotSupportedError::new("window dragging is not supported on OHOS").into())
    }

    fn drag_resize_window(&self, _direction: ResizeDirection) -> Result<(), RequestError> {
        Err(NotSupportedError::new("drag resize is not supported on OHOS").into())
    }

    fn show_window_menu(&self, _position: Position) {}

    fn set_cursor_hittest(&self, _hittest: bool) -> Result<(), RequestError> {
        Err(NotSupportedError::new("cursor hit-testing is not supported on OHOS").into())
    }

    fn set_theme(&self, _theme: Option<Theme>) {}

    fn theme(&self) -> Option<Theme> {
        None
    }

    fn set_content_protected(&self, _protected: bool) {}

    fn has_focus(&self) -> bool {
        self.shared.inner.lock().unwrap().focused
    }

    fn title(&self) -> String {
        String::from("tgui-winit-ohos")
    }

    fn reset_dead_keys(&self) {}

    fn rwh_06_display_handle(&self) -> &dyn rwh_06::HasDisplayHandle {
        self
    }

    fn rwh_06_window_handle(&self) -> &dyn rwh_06::HasWindowHandle {
        self
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        self.ohos_app.shutdown();
        EVENT_LOOP_CREATED.store(false, Ordering::Release);
    }
}

fn sanitize_scale_factor(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

fn sanitize_font_scale(font_scale: f64) -> f64 {
    if font_scale.is_finite() && font_scale > 0.0 {
        font_scale
    } else {
        1.0
    }
}

fn update_surface_state(state: &mut WindowInner, event: SurfaceEvent) {
    state.xcomponent = (event.xcomponent != 0).then_some(event.xcomponent);
    state.native_window = (event.native_window != 0).then_some(event.native_window);
    state.surface_size = PhysicalSize::new(event.width, event.height);
    state.scale_factor = sanitize_scale_factor(event.scale_factor);
    state.font_scale = sanitize_font_scale(event.font_scale);
}

fn min_timeout(lhs: Option<Duration>, rhs: Option<Duration>) -> Option<Duration> {
    lhs.map_or(rhs, |lhs| rhs.map_or(Some(lhs), |rhs| Some(lhs.min(rhs))))
}

fn map_mouse_button(button: OhosMouseButton) -> MouseButton {
    match button {
        OhosMouseButton::Left => MouseButton::Left,
        OhosMouseButton::Middle => MouseButton::Middle,
        OhosMouseButton::Right => MouseButton::Right,
        OhosMouseButton::Back => MouseButton::Back,
        OhosMouseButton::Forward => MouseButton::Forward,
    }
}

fn update_modifiers(
    shared: &Arc<SharedWindowState>,
    key_code: u32,
    state: event::ElementState,
) -> Option<Modifiers> {
    let modifier = match key_code {
        2047 => Some((ModifiersState::SHIFT, ModifiersKeys::LSHIFT)),
        2048 => Some((ModifiersState::SHIFT, ModifiersKeys::RSHIFT)),
        2045 => Some((ModifiersState::ALT, ModifiersKeys::LALT)),
        2046 => Some((ModifiersState::ALT, ModifiersKeys::RALT)),
        2072 => Some((ModifiersState::CONTROL, ModifiersKeys::LCONTROL)),
        2073 => Some((ModifiersState::CONTROL, ModifiersKeys::RCONTROL)),
        2076 => Some((ModifiersState::META, ModifiersKeys::LMETA)),
        2077 => Some((ModifiersState::META, ModifiersKeys::RMETA)),
        _ => None,
    }?;

    let mut inner = shared.inner.lock().unwrap();
    let before = Modifiers::new(inner.modifiers, inner.modifier_keys);

    if state.is_pressed() {
        inner.modifiers.insert(modifier.0);
        inner.modifier_keys.insert(modifier.1);
    } else {
        inner.modifiers.remove(modifier.0);
        inner.modifier_keys.remove(modifier.1);
    }

    let after = Modifiers::new(inner.modifiers, inner.modifier_keys);
    if before == after { None } else { Some(after) }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Mutex, OnceLock};

    use crate::WindowExtOhos;
    use raw_window_handle::{HandleError, HasWindowHandle};
    use winit_core::event_loop::pump_events::PumpStatus;

    #[derive(Default)]
    struct RecordingApp {
        events: Vec<WindowEvent>,
        resumed: usize,
        suspended: usize,
        can_create_surfaces: usize,
        destroy_surfaces: usize,
        proxy_wake_ups: usize,
        memory_warnings: usize,
        window: Option<Box<dyn CoreWindow>>,
        exit_on_about_to_wait: bool,
    }

    impl ApplicationHandler for RecordingApp {
        fn resumed(&mut self, _event_loop: &dyn RootActiveEventLoop) {
            self.resumed += 1;
        }

        fn can_create_surfaces(&mut self, event_loop: &dyn RootActiveEventLoop) {
            self.can_create_surfaces += 1;
            if self.window.is_none() {
                self.window = Some(
                    event_loop
                        .create_window(WindowAttributes::default())
                        .unwrap(),
                );
            }
        }

        fn proxy_wake_up(&mut self, _event_loop: &dyn RootActiveEventLoop) {
            self.proxy_wake_ups += 1;
        }

        fn memory_warning(&mut self, _event_loop: &dyn RootActiveEventLoop) {
            self.memory_warnings += 1;
        }

        fn window_event(
            &mut self,
            _event_loop: &dyn RootActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            self.events.push(event);
        }

        fn about_to_wait(&mut self, event_loop: &dyn RootActiveEventLoop) {
            if self.exit_on_about_to_wait {
                event_loop.exit();
            }
        }

        fn suspended(&mut self, _event_loop: &dyn RootActiveEventLoop) {
            self.suspended += 1;
        }

        fn destroy_surfaces(&mut self, _event_loop: &dyn RootActiveEventLoop) {
            self.destroy_surfaces += 1;
        }
    }

    fn build_event_loop(app: OhosApp) -> EventLoop {
        let mut builder = EventLoop::builder();
        builder.with_ohos_app(app);
        builder.build().unwrap()
    }

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static TEST_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn lifecycle_updates_surface_and_callbacks() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let mut event_loop = build_event_loop(app.clone());
        let mut recording = RecordingApp::default();

        app.notify_surface_created(
            1usize as *mut c_void,
            2usize as *mut c_void,
            640,
            480,
            2.0,
            1.35,
        );
        assert_eq!(
            event_loop.pump_app_events(Some(Duration::ZERO), &mut recording),
            PumpStatus::Continue
        );

        assert_eq!(recording.resumed, 1);
        assert_eq!(recording.can_create_surfaces, 1);
        assert!(matches!(
            recording.events.get(0),
            Some(WindowEvent::ScaleFactorChanged { scale_factor, .. }) if (*scale_factor - 2.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            recording.events.get(1),
            Some(WindowEvent::SurfaceResized(size)) if *size == PhysicalSize::new(640, 480)
        ));

        {
            let window = recording.window.as_ref().unwrap();
            assert_eq!(window.native_window_ptr() as usize, 2);
            assert_eq!(window.xcomponent_ptr() as usize, 1);
            assert!((window.font_scale() - 1.35).abs() < f64::EPSILON);
            assert!(window.window_handle().is_ok());
        }

        app.notify_surface_destroyed();
        assert_eq!(
            event_loop.pump_app_events(Some(Duration::ZERO), &mut recording),
            PumpStatus::Continue
        );

        assert_eq!(recording.destroy_surfaces, 1);
        assert_eq!(recording.suspended, 1);
        assert!(matches!(
            recording.window.as_ref().unwrap().window_handle(),
            Err(HandleError::Unavailable)
        ));
    }

    #[test]
    fn input_events_are_mapped_to_window_events() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let mut event_loop = build_event_loop(app.clone());
        let mut recording = RecordingApp::default();

        app.notify_surface_created(
            1usize as *mut c_void,
            2usize as *mut c_void,
            200,
            120,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        recording.events.clear();

        app.notify_focus(true);
        app.notify_touch(
            OhosTouchAction::Down,
            OhosPointerSource::Touchscreen,
            11,
            12.0,
            13.0,
            Some(0.5),
            7,
            true,
        );
        app.notify_touch(
            OhosTouchAction::Move,
            OhosPointerSource::Touchscreen,
            11,
            14.0,
            15.0,
            Some(0.5),
            7,
            true,
        );
        app.notify_mouse(
            OhosMouseAction::ButtonDown,
            Some(OhosMouseButton::Left),
            9.0,
            8.0,
            0.0,
            0.0,
            3,
            true,
        );
        app.notify_key(OhosKeyAction::Down, 2047, false, 5);

        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);

        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::Focused(true)))
        );
        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::PointerEntered { .. }))
        );
        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::PointerMoved { .. }))
        );
        assert!(recording.events.iter().any(|event| matches!(
            event,
            WindowEvent::PointerButton {
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            }
        )));
        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::ModifiersChanged(_)))
        );
        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::KeyboardInput { .. }))
        );
    }

    #[test]
    fn surface_recreation_only_resumes_once_per_lifecycle() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let mut event_loop = build_event_loop(app.clone());
        let mut recording = RecordingApp::default();

        app.notify_surface_created(
            1usize as *mut c_void,
            2usize as *mut c_void,
            640,
            480,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        recording.events.clear();

        app.notify_surface_created(
            1usize as *mut c_void,
            2usize as *mut c_void,
            640,
            480,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        assert_eq!(recording.resumed, 1);
        assert_eq!(recording.can_create_surfaces, 1);

        app.notify_surface_destroyed();
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        app.notify_surface_created(
            3usize as *mut c_void,
            4usize as *mut c_void,
            320,
            240,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);

        assert_eq!(recording.resumed, 2);
        assert_eq!(recording.can_create_surfaces, 2);
        assert!(recording.window.as_ref().unwrap().window_handle().is_ok());
    }

    #[test]
    fn redraw_requests_are_gated_by_frame_callbacks() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let mut event_loop = build_event_loop(app.clone());
        let mut recording = RecordingApp::default();

        app.notify_surface_created(
            1usize as *mut c_void,
            2usize as *mut c_void,
            200,
            120,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        recording.events.clear();

        recording.window.as_ref().unwrap().request_redraw();
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        assert!(
            !recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::RedrawRequested))
        );

        app.notify_frame();
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        assert_eq!(
            recording
                .events
                .iter()
                .filter(|event| matches!(event, WindowEvent::RedrawRequested))
                .count(),
            1
        );
    }

    #[test]
    fn proxy_wakeups_are_merged_and_exit_is_reported() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let mut event_loop = build_event_loop(app);
        let proxy = event_loop.create_proxy();
        let mut recording = RecordingApp {
            exit_on_about_to_wait: true,
            ..Default::default()
        };

        proxy.wake_up();
        proxy.wake_up();

        assert_eq!(
            event_loop.pump_app_events(Some(Duration::ZERO), &mut recording),
            PumpStatus::Exit(0)
        );
        assert_eq!(recording.proxy_wake_ups, 1);
    }

    #[test]
    fn raw_window_handle_is_unavailable_before_surface_exists() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let event_loop = build_event_loop(app.clone());
        let window = event_loop
            .window_target
            .create_window(WindowAttributes::default())
            .unwrap();

        assert!(matches!(
            window.window_handle(),
            Err(HandleError::Unavailable)
        ));

        drop(window);
        drop(event_loop);

        let mut event_loop = build_event_loop(app.clone());
        let mut recording = RecordingApp::default();
        app.notify_surface_created(
            7usize as *mut c_void,
            9usize as *mut c_void,
            32,
            32,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        assert!(recording.window.as_ref().unwrap().window_handle().is_ok());
    }

    #[test]
    fn default_builder_injects_an_ohos_app() {
        let _guard = test_guard();
        let event_loop = EventLoop::builder().build().unwrap();
        let _app = event_loop.ohos_app().clone();
    }

    #[test]
    fn memory_warnings_and_visibility_do_not_break_state() {
        let _guard = test_guard();
        let app = OhosApp::new();
        let mut event_loop = build_event_loop(app.clone());
        let mut recording = RecordingApp::default();

        app.notify_surface_created(
            1usize as *mut c_void,
            2usize as *mut c_void,
            200,
            120,
            1.0,
            1.0,
        );
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);
        recording.events.clear();

        app.notify_visibility(false);
        app.notify_low_memory();
        app.notify_visibility(true);
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut recording);

        assert_eq!(recording.memory_warnings, 1);
        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::Occluded(true)))
        );
        assert!(
            recording
                .events
                .iter()
                .any(|event| matches!(event, WindowEvent::Occluded(false)))
        );
        assert!(recording.window.as_ref().unwrap().window_handle().is_ok());
    }
}
