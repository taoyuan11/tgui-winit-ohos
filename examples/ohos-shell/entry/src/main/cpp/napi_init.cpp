#include <napi/native_api.h>
#include <stddef.h>
#include <stdint.h>

#include <mutex>
#include <string>
#include <vector>

#include <ace/xcomponent/native_interface_xcomponent.h>

#include "ohos_smoke.h"

namespace {

std::mutex g_runtimeMutex;
SmokeRuntime* g_runtime = nullptr;
float g_lastMouseX = 0.0f;
float g_lastMouseY = 0.0f;

SmokeRuntime* EnsureRuntime()
{
    std::lock_guard<std::mutex> lock(g_runtimeMutex);
    if (g_runtime == nullptr) {
        g_runtime = ohos_smoke_runtime_new();
    }
    return g_runtime;
}

uint32_t MapTouchAction(OH_NativeXComponent_TouchEventType type)
{
    switch (type) {
        case OH_NATIVEXCOMPONENT_DOWN:
            return 0;
        case OH_NATIVEXCOMPONENT_UP:
            return 1;
        case OH_NATIVEXCOMPONENT_MOVE:
            return 2;
        case OH_NATIVEXCOMPONENT_CANCEL:
        default:
            return 3;
    }
}

uint32_t MapPointerSource(OH_NativeXComponent_EventSourceType source)
{
    switch (source) {
        case OH_NATIVEXCOMPONENT_SOURCE_TYPE_TOUCHSCREEN:
            return 0;
        case OH_NATIVEXCOMPONENT_SOURCE_TYPE_MOUSE:
            return 1;
        case OH_NATIVEXCOMPONENT_SOURCE_TYPE_TOUCHPAD:
            return 2;
        default:
            return 3;
    }
}

uint32_t MapMouseAction(OH_NativeXComponent_MouseEventAction action)
{
    switch (action) {
        case OH_NATIVEXCOMPONENT_MOUSE_PRESS:
            return 1;
        case OH_NATIVEXCOMPONENT_MOUSE_RELEASE:
            return 2;
        case OH_NATIVEXCOMPONENT_MOUSE_MOVE:
            return 0;
        case OH_NATIVEXCOMPONENT_MOUSE_CANCEL:
            return 6;
        case OH_NATIVEXCOMPONENT_MOUSE_NONE:
        default:
            return 0;
    }
}

uint32_t MapMouseButton(OH_NativeXComponent_MouseEventButton button, bool* hasButton)
{
    *hasButton = true;
    switch (button) {
        case OH_NATIVEXCOMPONENT_LEFT_BUTTON:
            return 0;
        case OH_NATIVEXCOMPONENT_MIDDLE_BUTTON:
            return 1;
        case OH_NATIVEXCOMPONENT_RIGHT_BUTTON:
            return 2;
        case OH_NATIVEXCOMPONENT_BACK_BUTTON:
            return 3;
        case OH_NATIVEXCOMPONENT_FORWARD_BUTTON:
            return 4;
        case OH_NATIVEXCOMPONENT_NONE_BUTTON:
        default:
            *hasButton = false;
            return 0;
    }
}

uint32_t MapKeyAction(OH_NativeXComponent_KeyAction action)
{
    switch (action) {
        case OH_NATIVEXCOMPONENT_KEY_ACTION_DOWN:
            return 0;
        case OH_NATIVEXCOMPONENT_KEY_ACTION_UP:
            return 1;
        case OH_NATIVEXCOMPONENT_KEY_ACTION_UNKNOWN:
        default:
            return 2;
    }
}

std::string ReadMessage()
{
    SmokeRuntime* runtime = EnsureRuntime();
    size_t required = ohos_smoke_runtime_copy_message(runtime, nullptr, 0);
    std::vector<uint8_t> buffer(required + 1, 0);
    if (required > 0) {
        ohos_smoke_runtime_copy_message(runtime, buffer.data(), buffer.size());
    }
    return std::string(reinterpret_cast<const char*>(buffer.data()));
}

void OnSurfaceCreated(OH_NativeXComponent* component, void* window)
{
    uint64_t width = 0;
    uint64_t height = 0;
    OH_NativeXComponent_GetXComponentSize(component, window, &width, &height);
    ohos_smoke_runtime_surface_created(
        EnsureRuntime(), component, window, static_cast<uint32_t>(width), static_cast<uint32_t>(height), 1.0);
}

void OnSurfaceChanged(OH_NativeXComponent* component, void* window)
{
    uint64_t width = 0;
    uint64_t height = 0;
    OH_NativeXComponent_GetXComponentSize(component, window, &width, &height);
    ohos_smoke_runtime_surface_changed(
        EnsureRuntime(), component, window, static_cast<uint32_t>(width), static_cast<uint32_t>(height), 1.0);
}

void OnSurfaceDestroyed(OH_NativeXComponent* component, void* window)
{
    (void)component;
    (void)window;
    ohos_smoke_runtime_surface_destroyed(EnsureRuntime());
}

void OnTouch(OH_NativeXComponent* component, void* window)
{
    OH_NativeXComponent_TouchEvent touchEvent {};
    if (OH_NativeXComponent_GetTouchEvent(component, window, &touchEvent) != 0) {
        return;
    }

    OH_NativeXComponent_EventSourceType source = OH_NATIVEXCOMPONENT_SOURCE_TYPE_TOUCHSCREEN;
    OH_NativeXComponent_GetTouchEventSourceType(component, touchEvent.id, &source);

    bool primary = touchEvent.numPoints == 0 || touchEvent.touchPoints[0].id == touchEvent.id;
    ohos_smoke_runtime_touch(
        EnsureRuntime(),
        MapTouchAction(touchEvent.type),
        MapPointerSource(source),
        static_cast<uint64_t>(touchEvent.id),
        touchEvent.x,
        touchEvent.y,
        touchEvent.force,
        true,
        touchEvent.deviceId,
        primary);
}

void OnMouse(OH_NativeXComponent* component, void* window)
{
    OH_NativeXComponent_MouseEvent mouseEvent {};
    if (OH_NativeXComponent_GetMouseEvent(component, window, &mouseEvent) != 0) {
        return;
    }

    g_lastMouseX = mouseEvent.x;
    g_lastMouseY = mouseEvent.y;

    bool hasButton = false;
    uint32_t button = MapMouseButton(mouseEvent.button, &hasButton);
    ohos_smoke_runtime_mouse(
        EnsureRuntime(),
        MapMouseAction(mouseEvent.action),
        button,
        hasButton,
        mouseEvent.x,
        mouseEvent.y,
        0.0,
        0.0,
        0,
        true);
}

void OnHover(OH_NativeXComponent* component, bool isHover)
{
    (void)component;
    ohos_smoke_runtime_mouse(
        EnsureRuntime(),
        isHover ? 4 : 5,
        0,
        false,
        g_lastMouseX,
        g_lastMouseY,
        0.0,
        0.0,
        0,
        true);
}

void OnFocus(OH_NativeXComponent* component, void* window)
{
    (void)component;
    (void)window;
    ohos_smoke_runtime_focus(EnsureRuntime(), true);
}

void OnBlur(OH_NativeXComponent* component, void* window)
{
    (void)component;
    (void)window;
    ohos_smoke_runtime_focus(EnsureRuntime(), false);
}

void OnKey(OH_NativeXComponent* component, void* window)
{
    (void)window;
    OH_NativeXComponent_KeyEvent* keyEvent = nullptr;
    if (OH_NativeXComponent_GetKeyEvent(component, &keyEvent) != 0 || keyEvent == nullptr) {
        return;
    }

    OH_NativeXComponent_KeyAction action = OH_NATIVEXCOMPONENT_KEY_ACTION_UNKNOWN;
    OH_NativeXComponent_KeyCode code = static_cast<OH_NativeXComponent_KeyCode>(KEY_UNKNOWN);
    int64_t deviceId = 0;
    OH_NativeXComponent_GetKeyEventAction(keyEvent, &action);
    OH_NativeXComponent_GetKeyEventCode(keyEvent, &code);
    OH_NativeXComponent_GetKeyEventDeviceId(keyEvent, &deviceId);

    ohos_smoke_runtime_key(
        EnsureRuntime(),
        MapKeyAction(action),
        static_cast<uint32_t>(code),
        false,
        deviceId);
}

void OnSurfaceShow(OH_NativeXComponent* component, void* window)
{
    (void)component;
    (void)window;
    ohos_smoke_runtime_visibility(EnsureRuntime(), true);
}

void OnSurfaceHide(OH_NativeXComponent* component, void* window)
{
    (void)component;
    (void)window;
    ohos_smoke_runtime_visibility(EnsureRuntime(), false);
}

void OnFrame(OH_NativeXComponent* component, uint64_t timestamp, uint64_t targetTimestamp)
{
    (void)component;
    (void)timestamp;
    (void)targetTimestamp;
    ohos_smoke_runtime_frame(EnsureRuntime());
}

napi_value GetMessage(napi_env env, napi_callback_info info)
{
    (void)info;
    std::string message = ReadMessage();
    napi_value result = nullptr;
    napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &result);
    return result;
}

napi_value GetEventCount(napi_env env, napi_callback_info info)
{
    (void)info;
    napi_value result = nullptr;
    napi_create_uint32(env, static_cast<uint32_t>(ohos_smoke_runtime_event_count(EnsureRuntime())), &result);
    return result;
}

napi_value GetRedrawCount(napi_env env, napi_callback_info info)
{
    (void)info;
    napi_value result = nullptr;
    napi_create_uint32(env, static_cast<uint32_t>(ohos_smoke_runtime_redraw_count(EnsureRuntime())), &result);
    return result;
}

void RegisterXComponentCallbacks(napi_env env, napi_value exports)
{
    napi_value exportInstance = nullptr;
    if (napi_get_named_property(env, exports, OH_NATIVE_XCOMPONENT_OBJ, &exportInstance) != napi_ok) {
        return;
    }

    OH_NativeXComponent* nativeXComponent = nullptr;
    if (napi_unwrap(env, exportInstance, reinterpret_cast<void**>(&nativeXComponent)) != napi_ok ||
        nativeXComponent == nullptr) {
        return;
    }

    static OH_NativeXComponent_Callback componentCallbacks {
        .OnSurfaceCreated = OnSurfaceCreated,
        .OnSurfaceChanged = OnSurfaceChanged,
        .OnSurfaceDestroyed = OnSurfaceDestroyed,
        .DispatchTouchEvent = OnTouch,
    };

    static OH_NativeXComponent_MouseEvent_Callback mouseCallbacks {
        .DispatchMouseEvent = OnMouse,
        .DispatchHoverEvent = OnHover,
    };

    OH_NativeXComponent_RegisterCallback(nativeXComponent, &componentCallbacks);
    OH_NativeXComponent_RegisterMouseEventCallback(nativeXComponent, &mouseCallbacks);
    OH_NativeXComponent_RegisterFocusEventCallback(nativeXComponent, OnFocus);
    OH_NativeXComponent_RegisterBlurEventCallback(nativeXComponent, OnBlur);
    OH_NativeXComponent_RegisterKeyEventCallback(nativeXComponent, OnKey);
    OH_NativeXComponent_RegisterSurfaceShowCallback(nativeXComponent, OnSurfaceShow);
    OH_NativeXComponent_RegisterSurfaceHideCallback(nativeXComponent, OnSurfaceHide);
    OH_NativeXComponent_RegisterOnFrameCallback(nativeXComponent, OnFrame);
}

napi_value Init(napi_env env, napi_value exports)
{
    napi_property_descriptor descriptors[] = {
        { "getMessage", nullptr, GetMessage, nullptr, nullptr, nullptr, napi_default, nullptr },
        { "getEventCount", nullptr, GetEventCount, nullptr, nullptr, nullptr, napi_default, nullptr },
        { "getRedrawCount", nullptr, GetRedrawCount, nullptr, nullptr, nullptr, napi_default, nullptr },
    };
    napi_define_properties(env, exports, sizeof(descriptors) / sizeof(descriptors[0]), descriptors);
    RegisterXComponentCallbacks(env, exports);
    return exports;
}

static napi_module g_module = {
    .nm_version = 1,
    .nm_flags = 0,
    .nm_filename = nullptr,
    .nm_register_func = Init,
    .nm_modname = "entry",
    .nm_priv = nullptr,
    .reserved = { 0 },
};

} // namespace

extern "C" __attribute__((constructor)) void RegisterEntryModule(void)
{
    napi_module_register(&g_module);
}
