use std::collections::VecDeque;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct OhosApp {
    inner: Arc<OhosAppInner>,
}

impl PartialEq for OhosApp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for OhosApp {}

impl Hash for OhosApp {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

#[derive(Debug)]
struct OhosAppInner {
    queue: Mutex<VecDeque<HostEvent>>,
    cv: Condvar,
    shutdown: AtomicBool,
}

#[derive(Debug, Clone)]
pub(crate) enum HostEvent {
    SurfaceCreated(SurfaceEvent),
    SurfaceChanged(SurfaceEvent),
    SurfaceDestroyed,
    Focused(bool),
    Visible(bool),
    LowMemory,
    FrameAvailable,
    RedrawRequested,
    ProxyWake,
    Touch(TouchEvent),
    Mouse(MouseEvent),
    Key(KeyEvent),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceEvent {
    pub xcomponent: usize,
    pub native_window: usize,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TouchEvent {
    pub action: OhosTouchAction,
    pub source: OhosPointerSource,
    pub finger_id: u64,
    pub x: f64,
    pub y: f64,
    pub force: Option<f64>,
    pub device_id: i64,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MouseEvent {
    pub action: OhosMouseAction,
    pub button: Option<OhosMouseButton>,
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    pub device_id: i64,
    pub primary: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct KeyEvent {
    pub action: OhosKeyAction,
    pub key_code: u32,
    pub repeat: bool,
    pub device_id: i64,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhosTouchAction {
    Down = 0,
    Up = 1,
    Move = 2,
    Cancel = 3,
}

impl OhosTouchAction {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::Move),
            3 => Some(Self::Cancel),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhosMouseAction {
    Move = 0,
    ButtonDown = 1,
    ButtonUp = 2,
    Wheel = 3,
    Enter = 4,
    Leave = 5,
    Cancel = 6,
}

impl OhosMouseAction {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Move),
            1 => Some(Self::ButtonDown),
            2 => Some(Self::ButtonUp),
            3 => Some(Self::Wheel),
            4 => Some(Self::Enter),
            5 => Some(Self::Leave),
            6 => Some(Self::Cancel),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhosMouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    Back = 3,
    Forward = 4,
}

impl OhosMouseButton {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Left),
            1 => Some(Self::Middle),
            2 => Some(Self::Right),
            3 => Some(Self::Back),
            4 => Some(Self::Forward),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhosKeyAction {
    Down = 0,
    Up = 1,
    Unknown = 2,
}

impl OhosKeyAction {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhosPointerSource {
    Touchscreen = 0,
    Mouse = 1,
    Touchpad = 2,
    Unknown = 3,
}

impl OhosPointerSource {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Touchscreen),
            1 => Some(Self::Mouse),
            2 => Some(Self::Touchpad),
            3 => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl Default for OhosApp {
    fn default() -> Self {
        Self::new()
    }
}

impl OhosApp {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OhosAppInner {
                queue: Mutex::new(VecDeque::new()),
                cv: Condvar::new(),
                shutdown: AtomicBool::new(false),
            }),
        }
    }

    pub fn notify_surface_created(
        &self,
        xcomponent: *mut c_void,
        native_window: *mut c_void,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) {
        self.push(HostEvent::SurfaceCreated(SurfaceEvent {
            xcomponent: xcomponent as usize,
            native_window: native_window as usize,
            width,
            height,
            scale_factor,
        }));
    }

    pub fn notify_surface_changed(
        &self,
        xcomponent: *mut c_void,
        native_window: *mut c_void,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) {
        self.push(HostEvent::SurfaceChanged(SurfaceEvent {
            xcomponent: xcomponent as usize,
            native_window: native_window as usize,
            width,
            height,
            scale_factor,
        }));
    }

    pub fn notify_surface_destroyed(&self) {
        self.push(HostEvent::SurfaceDestroyed);
    }

    pub fn notify_focus(&self, focused: bool) {
        self.push(HostEvent::Focused(focused));
    }

    pub fn notify_visibility(&self, visible: bool) {
        self.push(HostEvent::Visible(visible));
    }

    pub fn notify_low_memory(&self) {
        self.push(HostEvent::LowMemory);
    }

    pub fn notify_frame(&self) {
        self.push(HostEvent::FrameAvailable);
    }

    pub fn notify_key(&self, action: OhosKeyAction, key_code: u32, repeat: bool, device_id: i64) {
        self.push(HostEvent::Key(KeyEvent {
            action,
            key_code,
            repeat,
            device_id,
        }));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn notify_touch(
        &self,
        action: OhosTouchAction,
        source: OhosPointerSource,
        finger_id: u64,
        x: f64,
        y: f64,
        force: Option<f64>,
        device_id: i64,
        primary: bool,
    ) {
        self.push(HostEvent::Touch(TouchEvent {
            action,
            source,
            finger_id,
            x,
            y,
            force,
            device_id,
            primary,
        }));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn notify_mouse(
        &self,
        action: OhosMouseAction,
        button: Option<OhosMouseButton>,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        device_id: i64,
        primary: bool,
    ) {
        self.push(HostEvent::Mouse(MouseEvent {
            action,
            button,
            x,
            y,
            delta_x,
            delta_y,
            device_id,
            primary,
        }));
    }

    pub(crate) fn wake_proxy(&self) {
        self.push(HostEvent::ProxyWake);
    }

    pub(crate) fn request_redraw(&self) {
        self.push(HostEvent::RedrawRequested);
    }

    pub(crate) fn wait_and_drain(&self, timeout: Option<Duration>) -> Vec<HostEvent> {
        let mut queue = self.inner.queue.lock().unwrap();

        if queue.is_empty() && !self.inner.shutdown.load(Ordering::Acquire) {
            match timeout {
                Some(timeout) if timeout.is_zero() => {}
                Some(timeout) => {
                    let start = Instant::now();
                    let mut remaining = timeout;
                    while queue.is_empty()
                        && !self.inner.shutdown.load(Ordering::Acquire)
                        && !remaining.is_zero()
                    {
                        let (next_queue, _) = self.inner.cv.wait_timeout(queue, remaining).unwrap();
                        queue = next_queue;
                        remaining = timeout.saturating_sub(start.elapsed());
                    }
                }
                None => {
                    while queue.is_empty() && !self.inner.shutdown.load(Ordering::Acquire) {
                        queue = self.inner.cv.wait(queue).unwrap();
                    }
                }
            }
        }

        queue.drain(..).collect()
    }

    pub(crate) fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::Release);
        self.inner.cv.notify_all();
    }

    fn push(&self, event: HostEvent) {
        let mut queue = self.inner.queue.lock().unwrap();
        queue.push_back(event);
        drop(queue);
        self.inner.cv.notify_all();
    }
}
