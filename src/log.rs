use std::ffi::{CStr, c_char};

#[cfg(any(target_env = "ohos", test))]
use std::borrow::Cow;
#[cfg(target_env = "ohos")]
use std::ffi::{CString, c_int};

pub const LOG_PREFIX: &str = "rust => ";
pub const DEFAULT_LOG_DOMAIN: u32 = 0x3433;
pub const DEFAULT_LOG_TAG: &str = "tgui-winit-ohos";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum OhosLogLevel {
    Debug = 3,
    Info = 4,
    Warn = 5,
    Error = 6,
    Fatal = 7,
}

pub fn deveco_log(message: impl AsRef<str>) {
    deveco_log_with_level(OhosLogLevel::Info, message);
}

pub fn deveco_log_with_level(level: OhosLogLevel, message: impl AsRef<str>) {
    deveco_log_with(level, DEFAULT_LOG_DOMAIN, DEFAULT_LOG_TAG, message);
}

pub fn deveco_log_with(
    level: OhosLogLevel,
    domain: u32,
    tag: impl AsRef<str>,
    message: impl AsRef<str>,
) {
    let tag = tag.as_ref();
    let domain = normalize_log_domain(domain);
    let formatted = format_message(message.as_ref());

    #[cfg(target_env = "ohos")]
    unsafe {
        let tag = sanitize_for_cstring(tag);
        let tag = CString::new(tag.as_bytes()).expect("tag was sanitized");
        let formatted = sanitize_for_cstring(&formatted);
        let message = CString::new(formatted.as_bytes()).expect("message was sanitized");
        let _ = cargo_ohos_app_hilog(level as u32, domain, tag.as_ptr(), message.as_ptr());
    }

    #[cfg(not(target_env = "ohos"))]
    eprintln!("[{level:?}][0x{domain:04x}] {tag}: {formatted}");
}

/// # Safety
///
/// `message` must either be null or point to a valid NUL-terminated C string.
pub unsafe fn deveco_log_from_c(message: *const c_char) {
    if message.is_null() {
        deveco_log("");
        return;
    }

    let message = unsafe { CStr::from_ptr(message) };
    deveco_log(message.to_string_lossy());
}

fn format_message(message: &str) -> String {
    format!("{LOG_PREFIX}{message}")
}

fn normalize_log_domain(domain: u32) -> u32 {
    if domain <= 0xFFFF {
        domain
    } else {
        DEFAULT_LOG_DOMAIN
    }
}

#[cfg(any(target_env = "ohos", test))]
fn sanitize_for_cstring(message: &str) -> Cow<'_, str> {
    if message.as_bytes().contains(&0) {
        Cow::Owned(message.replace('\0', " "))
    } else {
        Cow::Borrowed(message)
    }
}

#[cfg(target_env = "ohos")]
unsafe extern "C" {
    fn cargo_ohos_app_hilog(
        level: u32,
        domain: u32,
        tag: *const c_char,
        message: *const c_char,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_LOG_DOMAIN, OhosLogLevel, format_message, normalize_log_domain,
        sanitize_for_cstring,
    };

    #[test]
    fn prefixes_messages_for_deveco() {
        assert_eq!(format_message("hello"), "rust => hello");
    }

    #[test]
    fn sanitizes_embedded_nuls() {
        assert_eq!(sanitize_for_cstring("a\0b").as_ref(), "a b");
    }

    #[test]
    fn keeps_valid_log_domains() {
        assert_eq!(normalize_log_domain(0x1234), 0x1234);
    }

    #[test]
    fn falls_back_for_invalid_log_domains() {
        assert_eq!(normalize_log_domain(0x1_0000), DEFAULT_LOG_DOMAIN);
    }

    #[test]
    fn preserves_ohos_log_level_values() {
        assert_eq!(OhosLogLevel::Debug as u32, 3);
        assert_eq!(OhosLogLevel::Info as u32, 4);
        assert_eq!(OhosLogLevel::Warn as u32, 5);
        assert_eq!(OhosLogLevel::Error as u32, 6);
        assert_eq!(OhosLogLevel::Fatal as u32, 7);
    }
}
