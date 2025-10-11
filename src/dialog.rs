use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, msg_send_id};
use objc2_app_kit::NSAlert;
use objc2_foundation::{CGPoint, CGRect, CGSize, MainThreadMarker, NSString};

fn show_dialog_internal(mtm: MainThreadMarker) -> Option<String> {
    println!("show_dialog_internal called");

    unsafe {
        // Make sure NSApplication is initialized and activated
        let app_class = objc2::class!(NSApplication);
        let app: Retained<AnyObject> = msg_send_id![app_class, sharedApplication];
        let _: () = msg_send![&app, activateIgnoringOtherApps: true];

        // Create the alert
        let alert = NSAlert::new(mtm);

        println!("Alert created");

        // Set message and informative text
        alert.setMessageText(&NSString::from_str("Enter ICS URL"));
        alert.setInformativeText(&NSString::from_str(
            "Please enter the URL to your ICS calendar file:",
        ));

        // Add buttons using msg_send
        let continue_btn = NSString::from_str("Update");
        let _: Retained<AnyObject> = msg_send_id![&alert, addButtonWithTitle: &*continue_btn];

        let cancel_btn = NSString::from_str("Cancel");
        let _: Retained<AnyObject> = msg_send_id![&alert, addButtonWithTitle: &*cancel_btn];

        // Create text field
        let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(500.0, 24.0));
        let text_field_class = objc2::class!(NSTextField);
        let text_field: Retained<AnyObject> = msg_send_id![text_field_class, new];
        let _: () = msg_send![&text_field, setFrame: frame];

        // Set placeholder
        let placeholder = NSString::from_str("https://calendar.google.com/...");
        let _: () = msg_send![&text_field, setPlaceholderString: &*placeholder];

        // Add text field to alert as accessory view
        let _: () = msg_send![&alert, setAccessoryView: &*text_field];

        println!("About to show modal dialog");

        // Show dialog and get response
        let response: isize = msg_send![&alert, runModal];

        println!("Dialog response: {}", response);

        // NSAlertFirstButtonReturn = 1000
        if response == 1000 {
            // Get the entered text
            let ns_string: Retained<NSString> = msg_send_id![&text_field, stringValue];
            let rust_string = ns_string.to_string();

            if !rust_string.is_empty() {
                println!("Returning URL: {}", rust_string);
                return Some(rust_string);
            }
        }

        println!("Dialog cancelled or empty");
        None
    }
}

pub fn show_url_input_dialog() -> Option<String> {
    println!("show_url_input_dialog called");

    // Get the main thread marker (required for AppKit operations)
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    show_dialog_internal(mtm)
}
