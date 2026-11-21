// Microphone-activity detection via the CoreAudio hardware C API.
//
// Exposed to Rust as microphone_is_active() (declared in src/activity.rs).
// Complements camera detection: audio-only calls never turn the camera on.
#import <CoreAudio/CoreAudio.h>
#include <stdbool.h>
#include <stdlib.h>
#include <unistd.h>

// Fetches a variable-length array property of `object` into a malloc'd buffer
// (caller frees), storing the element count in `count`. Returns NULL on error
// or when the array is empty.
static void *copy_array_property(AudioObjectID object, AudioObjectPropertySelector selector,
                                 AudioObjectPropertyScope scope, UInt32 elementSize, UInt32 *count) {
    AudioObjectPropertyAddress address = {selector, scope, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(object, &address, 0, NULL, &size) != noErr || size == 0) {
        return NULL;
    }
    void *data = malloc(size);
    if (data == NULL) {
        return NULL;
    }
    if (AudioObjectGetPropertyData(object, &address, 0, NULL, &size, data) != noErr) {
        free(data);
        return NULL;
    }
    *count = size / elementSize;
    return data;
}

// Reads a UInt32 property of `object`, returning `fallback` on error.
static UInt32 get_u32_property(AudioObjectID object, AudioObjectPropertySelector selector,
                               AudioObjectPropertyScope scope, UInt32 fallback) {
    AudioObjectPropertyAddress address = {selector, scope, kAudioObjectPropertyElementMain};
    UInt32 value = 0;
    UInt32 size = sizeof(value);
    if (AudioObjectGetPropertyData(object, &address, 0, NULL, &size, &value) != noErr) {
        return fallback;
    }
    return value;
}

// macOS 14.2+: true if any other process is capturing audio input. Per-process
// state is exact, unlike the device flag below, which can't tell input from
// output on combined devices (AirPods, headsets).
API_AVAILABLE(macos(14.2))
static bool any_process_running_input(void) {
    UInt32 count = 0;
    AudioObjectID *processes = copy_array_property(kAudioObjectSystemObject, kAudioHardwarePropertyProcessObjectList,
                                                   kAudioObjectPropertyScopeGlobal, sizeof(AudioObjectID), &count);
    if (processes == NULL) {
        return false;
    }
    pid_t self = getpid();
    bool active = false;
    for (UInt32 i = 0; i < count && !active; i++) {
        // ignore ourselves, in case audio playback ever opens an input stream
        if ((pid_t)get_u32_property(processes[i], kAudioProcessPropertyPID, kAudioObjectPropertyScopeGlobal, 0) == self) {
            continue;
        }
        active = get_u32_property(processes[i], kAudioProcessPropertyIsRunningInput, kAudioObjectPropertyScopeGlobal,
                                  0) != 0;
    }
    free(processes);
    return active;
}

// Pre-14.2 fallback: true if any input-capable device is running somewhere.
// Foot-gun: a combined device playing output only (e.g. AirPods playing
// music, or our own speech) also counts as running.
static bool any_input_device_running(void) {
    UInt32 count = 0;
    AudioObjectID *devices = copy_array_property(kAudioObjectSystemObject, kAudioHardwarePropertyDevices,
                                                 kAudioObjectPropertyScopeGlobal, sizeof(AudioObjectID), &count);
    if (devices == NULL) {
        return false;
    }
    bool active = false;
    for (UInt32 i = 0; i < count && !active; i++) {
        UInt32 inputStreams = 0;
        void *streams = copy_array_property(devices[i], kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput,
                                            sizeof(AudioStreamID), &inputStreams);
        free(streams);
        if (inputStreams == 0) {
            continue;
        }
        active = get_u32_property(devices[i], kAudioDevicePropertyDeviceIsRunningSomewhere,
                                  kAudioObjectPropertyScopeGlobal, 0) != 0;
    }
    free(devices);
    return active;
}

// Returns true if the microphone is being captured by some process, i.e. a
// call (possibly audio-only) is likely live.
bool microphone_is_active(void) {
    if (@available(macOS 14.2, *)) {
        return any_process_running_input();
    }
    return any_input_device_running();
}
