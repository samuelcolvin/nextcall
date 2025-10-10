use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, msg_send_id};
use objc2_foundation::{NSArray, NSString};
use std::ffi::c_void;
use std::mem;

const KCMIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE: u32 = 0x676f6e65; // 'gone' in FourCC

#[repr(C)]
struct CMIOObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "AVFoundation", kind = "framework")]
#[link(name = "CoreMediaIO", kind = "framework")]
unsafe extern "C" {
    fn CMIOObjectGetPropertyData(
        object_id: u32,
        address: *const CMIOObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        data_size: u32,
        data_used: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

pub fn camera_active() -> bool {
    unsafe {
        // Get AVCaptureDevice class
        let av_capture_device_class = AnyClass::get("AVCaptureDevice")
            .expect("AVCaptureDevice class not found");

        // Create AVMediaTypeVideo NSString - the actual constant value is "vide"
        let av_media_type_video = NSString::from_str("vide");

        // Get all video devices
        let devices: Option<Retained<NSArray>> = msg_send_id![
            av_capture_device_class,
            devicesWithMediaType: &*av_media_type_video
        ];

        let devices = match devices {
            Some(d) => d,
            None => return false,
        };

        if devices.is_empty() {
            return false;
        }

        // Create property address for kCMIODevicePropertyDeviceIsRunningSomewhere
        // Python's CMIOObjectPropertyAddress(selector) defaults to scope=0, element=0
        let property_address = CMIOObjectPropertyAddress {
            selector: KCMIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE,
            scope: 0,
            element: 0,
        };

        for i in 0..devices.len() {
            let device: *const AnyObject = msg_send![&devices, objectAtIndex: i];

            // Get the connection ID
            let connection_id: u32 = msg_send![device, connectionID];

            // Query if the device is running
            let mut is_running: u32 = 0;
            let mut data_used: u32 = 0;

            let result = CMIOObjectGetPropertyData(
                connection_id,
                &property_address,
                0,
                std::ptr::null(),
                mem::size_of::<u32>() as u32,
                &mut data_used,
                &mut is_running as *mut u32 as *mut c_void,
            );

            if result == 0 && is_running != 0 {
                return true;
            }
        }

        false
    }
}
