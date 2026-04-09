cmake_minimum_required(VERSION 3.5.0)
project(tgui_winit_ohos_shell)

set(TGUI_WINIT_OHOS_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../../../../..")

if(CMAKE_OHOS_ARCH_ABI STREQUAL "arm64-v8a")
    set(RUST_TARGET "aarch64-unknown-linux-ohos")
elseif(CMAKE_OHOS_ARCH_ABI STREQUAL "x86_64")
    set(RUST_TARGET "x86_64-unknown-linux-ohos")
else()
    message(FATAL_ERROR "Unsupported OHOS ABI: ${CMAKE_OHOS_ARCH_ABI}")
endif()

set(RUST_LIB "${TGUI_WINIT_OHOS_ROOT}/target/${RUST_TARGET}/debug/libohos_smoke.a")
if(NOT EXISTS "${RUST_LIB}")
    message(FATAL_ERROR "Missing Rust smoke library at ${RUST_LIB}. Build examples/ohos-smoke first.")
endif()

add_definitions(-DOHOS_PLATFORM)

find_library(HILOG_LIB hilog_ndk.z)
find_library(ACE_NDK_LIB ace_ndk.z)
find_library(ACE_NAPI_LIB ace_napi.z)
find_library(UV_LIB uv)

add_library(rust_smoke STATIC IMPORTED GLOBAL)
set_target_properties(rust_smoke PROPERTIES
    IMPORTED_LOCATION "${RUST_LIB}"
)

add_library(entry SHARED napi_init.cpp)
target_include_directories(entry PRIVATE
    "${TGUI_WINIT_OHOS_ROOT}/examples/ohos-smoke/include"
)
set_target_properties(entry PROPERTIES
    LIBRARY_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/out"
)
target_link_libraries(entry PUBLIC
    rust_smoke
    ${HILOG_LIB}
    ${ACE_NDK_LIB}
    ${ACE_NAPI_LIB}
    ${UV_LIB}
    libnative_window.so
)
