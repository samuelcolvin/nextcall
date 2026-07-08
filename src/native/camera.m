// Camera-activity detection via the CoreMediaIO hardware C API.
//
// Exposed to Rust as camera_is_active() (declared in src/camera.rs). Used to
// avoid interrupting an ongoing call with notifications/speech. Enumerates
// CMIO devices directly (they are all video/camera devices) rather than going
// through AVFoundation, whose device->CMIO bridge (connectionID) was removed
// from recent SDKs.
#import <CoreMediaIO/CMIOHardware.h>
#include <stdbool.h>
#include <stdlib.h>

// Returns true if any video device reports it is in use by some process
// (kCMIODevicePropertyDeviceIsRunningSomewhere), i.e. a call is likely live.
bool camera_is_active(void) {
    // Ask the CMIO system object for the list of all device IDs.
    CMIOObjectPropertyAddress devicesAddress = {
        .mSelector = kCMIOHardwarePropertyDevices,
        .mScope = kCMIOObjectPropertyScopeGlobal,
        .mElement = kCMIOObjectPropertyElementMain,
    };
    UInt32 dataSize = 0;
    if (CMIOObjectGetPropertyDataSize(kCMIOObjectSystemObject, &devicesAddress, 0, NULL, &dataSize) !=
            kCMIOHardwareNoError ||
        dataSize == 0) {
        return false;
    }

    UInt32 deviceCount = dataSize / sizeof(CMIODeviceID);
    CMIODeviceID *devices = calloc(deviceCount, sizeof(CMIODeviceID));
    if (devices == NULL) {
        return false;
    }
    UInt32 dataUsed = 0;
    if (CMIOObjectGetPropertyData(kCMIOObjectSystemObject, &devicesAddress, 0, NULL, dataSize, &dataUsed, devices) !=
        kCMIOHardwareNoError) {
        free(devices);
        return false;
    }

    // Scope/element 0 (rather than the named wildcard constants) matches the
    // behaviour of the previous implementation and works in practice.
    CMIOObjectPropertyAddress runningAddress = {
        .mSelector = kCMIODevicePropertyDeviceIsRunningSomewhere,
        .mScope = 0,
        .mElement = 0,
    };

    bool active = false;
    for (UInt32 i = 0; i < deviceCount && !active; i++) {
        UInt32 isRunning = 0;
        UInt32 used = 0;
        if (CMIOObjectGetPropertyData(devices[i], &runningAddress, 0, NULL, sizeof(isRunning), &used, &isRunning) ==
                kCMIOHardwareNoError &&
            isRunning != 0) {
            active = true;
        }
    }
    free(devices);
    return active;
}
