// Narrow C ABI around Valve's pinned C++ interfaces. No static OpenVR linkage.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <openvr.h>
#include <cstdint>

namespace {
HMODULE library = nullptr;
vr::IVRSystem* system_api = nullptr;
vr::IVROverlay* overlays = nullptr;
using Shutdown = void (VR_CALLTYPE*)();
Shutdown shutdown_api = nullptr;
bool initialized = false;
}

extern "C" {
void xs_vr_close() {
    overlays = nullptr;
    system_api = nullptr;
    if (initialized && shutdown_api) shutdown_api();
    initialized = false;
    shutdown_api = nullptr;
    if (library) FreeLibrary(library);
    library = nullptr;
}

// Negative errors belong to this loader; positive errors are EVRInitError.
int xs_vr_open(const wchar_t* path) {
    if (library) return -4;
    library = LoadLibraryExW(path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!library) return -1;
    using Init = uint32_t (VR_CALLTYPE*)(vr::EVRInitError*, vr::EVRApplicationType, const char*);
    using Interface = void* (VR_CALLTYPE*)(const char*, vr::EVRInitError*);
    auto init = reinterpret_cast<Init>(GetProcAddress(library, "VR_InitInternal2"));
    auto get = reinterpret_cast<Interface>(GetProcAddress(library, "VR_GetGenericInterface"));
    shutdown_api = reinterpret_cast<Shutdown>(GetProcAddress(library, "VR_ShutdownInternal"));
    if (!init || !get || !shutdown_api) { xs_vr_close(); return -2; }
    vr::EVRInitError error = vr::VRInitError_None;
    init(&error, vr::VRApplication_Overlay, nullptr);
    if (error != vr::VRInitError_None) { xs_vr_close(); return static_cast<int>(error); }
    initialized = true;
    system_api = static_cast<vr::IVRSystem*>(get(vr::IVRSystem_Version, &error));
    if (!system_api || error != vr::VRInitError_None) {
        const int code = error ? static_cast<int>(error) : -3;
        xs_vr_close(); return code;
    }
    overlays = static_cast<vr::IVROverlay*>(get(vr::IVROverlay_Version, &error));
    if (!overlays || error != vr::VRInitError_None) {
        const int code = error ? static_cast<int>(error) : -3;
        xs_vr_close(); return code;
    }
    return 0;
}

int xs_vr_adapter() {
    int32_t index = -1;
    system_api->GetDXGIOutputInfo(&index);
    return index;
}

bool xs_vr_head(float* matrix) {
    vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount]{};
    system_api->GetDeviceToAbsoluteTrackingPose(vr::TrackingUniverseStanding, 0, poses,
        vr::k_unMaxTrackedDeviceCount);
    const auto& head = poses[vr::k_unTrackedDeviceIndex_Hmd];
    if (!head.bPoseIsValid || !head.bDeviceIsConnected) return false;
    for (int r = 0; r < 3; ++r)
        for (int c = 0; c < 4; ++c) matrix[r * 4 + c] = head.mDeviceToAbsoluteTracking.m[r][c];
    return true;
}

bool xs_vr_quit() {
    vr::VREvent_t event{};
    while (system_api->PollNextEvent(&event, sizeof(event))) {
        if (event.eventType == vr::VREvent_Quit) {
            system_api->AcknowledgeQuit_Exiting();
            return true;
        }
    }
    return false;
}

int xs_vr_create(const char* key, const char* title, uint64_t* handle) {
    return static_cast<int>(overlays->CreateOverlay(key, title, handle));
}
void xs_vr_destroy(uint64_t handle) {
    overlays->HideOverlay(handle);
    overlays->ClearOverlayTexture(handle);
    overlays->DestroyOverlay(handle);
}
int xs_vr_show(uint64_t handle, bool visible) {
    return static_cast<int>(visible ? overlays->ShowOverlay(handle) : overlays->HideOverlay(handle));
}
void xs_vr_clear(uint64_t handle) { overlays->ClearOverlayTexture(handle); }
int xs_vr_place(uint64_t handle, float width, const float* values) {
    vr::HmdMatrix34_t matrix{};
    for (int r = 0; r < 3; ++r)
        for (int c = 0; c < 4; ++c) matrix.m[r][c] = values[r * 4 + c];
    auto error = overlays->SetOverlayWidthInMeters(handle, width);
    if (error != vr::VROverlayError_None) return static_cast<int>(error);
    return static_cast<int>(overlays->SetOverlayTransformAbsolute(handle,
        vr::TrackingUniverseStanding, &matrix));
}
int xs_vr_submit(uint64_t handle, void* d3d_texture, float u_min, float u_max, bool linear) {
    vr::VRTextureBounds_t bounds{u_min, 0.0f, u_max, 1.0f};
    auto error = overlays->SetOverlayTextureBounds(handle, &bounds);
    if (error != vr::VROverlayError_None) return static_cast<int>(error);
    vr::Texture_t texture{d3d_texture, vr::TextureType_DirectX,
        linear ? vr::ColorSpace_Linear : vr::ColorSpace_Gamma};
    return static_cast<int>(overlays->SetOverlayTexture(handle, &texture));
}
}
