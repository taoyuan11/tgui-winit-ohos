#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct SmokeRuntime;

struct SmokeRuntime* ohos_smoke_runtime_new(void);
void ohos_smoke_runtime_free(struct SmokeRuntime* runtime);

size_t ohos_smoke_runtime_copy_message(
    const struct SmokeRuntime* runtime,
    uint8_t* buffer,
    size_t capacity
);

uint64_t ohos_smoke_runtime_event_count(const struct SmokeRuntime* runtime);
uint64_t ohos_smoke_runtime_redraw_count(const struct SmokeRuntime* runtime);

void ohos_smoke_runtime_surface_created(
    const struct SmokeRuntime* runtime,
    void* xcomponent,
    void* native_window,
    uint32_t width,
    uint32_t height,
    double scale_factor
);

void ohos_smoke_runtime_surface_changed(
    const struct SmokeRuntime* runtime,
    void* xcomponent,
    void* native_window,
    uint32_t width,
    uint32_t height,
    double scale_factor
);

void ohos_smoke_runtime_surface_destroyed(const struct SmokeRuntime* runtime);
void ohos_smoke_runtime_focus(const struct SmokeRuntime* runtime, bool focused);
void ohos_smoke_runtime_visibility(const struct SmokeRuntime* runtime, bool visible);
void ohos_smoke_runtime_low_memory(const struct SmokeRuntime* runtime);
void ohos_smoke_runtime_frame(const struct SmokeRuntime* runtime);

void ohos_smoke_runtime_key(
    const struct SmokeRuntime* runtime,
    uint32_t action,
    uint32_t key_code,
    bool repeat,
    int64_t device_id
);

void ohos_smoke_runtime_touch(
    const struct SmokeRuntime* runtime,
    uint32_t action,
    uint32_t source,
    uint64_t finger_id,
    double x,
    double y,
    double force,
    bool has_force,
    int64_t device_id,
    bool primary
);

void ohos_smoke_runtime_mouse(
    const struct SmokeRuntime* runtime,
    uint32_t action,
    uint32_t button,
    bool has_button,
    double x,
    double y,
    double delta_x,
    double delta_y,
    int64_t device_id,
    bool primary
);

#ifdef __cplusplus
}
#endif
