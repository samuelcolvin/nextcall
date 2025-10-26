use std::mem::MaybeUninit;
use std::ptr;
use std::sync::Once;

use block2::RcBlock;
use objc2::rc::Id;
use objc2::runtime::{AnyObject, Bool};
use objc2::{ClassType, DeclaredClass, class, declare_class, msg_send, msg_send_id, mutability};
use objc2_foundation::{NSError, NSObject, NSString};

// Link the UserNotifications framework
#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

// Declare our custom delegate class
declare_class!(
    struct NotificationDelegate;

    unsafe impl ClassType for NotificationDelegate {
        type Super = NSObject;
        type Mutability = mutability::InteriorMutable;
        const NAME: &'static str = "NotificationDelegate";
    }

    impl DeclaredClass for NotificationDelegate {}

    unsafe impl NotificationDelegate {
        // Called when a notification is delivered while the app is in foreground
        #[method(userNotificationCenter:willPresentNotification:withCompletionHandler:)]
        fn will_present_notification(
            &self,
            _center: &AnyObject,
            _notification: &AnyObject,
            completion_handler: &AnyObject,
        ) {
            // Call completion handler with UNNotificationPresentationOptionBanner | Sound | Badge = 7
            unsafe {
                let block_ptr = completion_handler as *const _ as *const u8;
                let invoke_ptr: extern "C" fn(*const u8, u64) =
                    *(block_ptr.add(16) as *const extern "C" fn(*const u8, u64));
                invoke_ptr(block_ptr, 7);
            }
        }

        // Called when user interacts with a notification
        #[method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:)]
        fn did_receive_notification_response(
            &self,
            _center: &AnyObject,
            response: &AnyObject,
            completion_handler: &AnyObject,
        ) {
            // Get the notification object from the response
            let notification: *mut AnyObject = unsafe { msg_send![response, notification] };
            let request: *mut AnyObject = unsafe { msg_send![notification, request] };
            let content: *mut AnyObject = unsafe { msg_send![request, content] };
            let user_info: *mut AnyObject = unsafe { msg_send![content, userInfo] };

            // Get the URL from userInfo dictionary
            let url_key = NSString::from_str("url");
            let url_value: *mut AnyObject = unsafe { msg_send![user_info, objectForKey: &*url_key] };

            if !url_value.is_null() {
                let url_nsstring = url_value as *mut NSString;
                let url_str = unsafe { (*url_nsstring).to_string() };

                // Open the URL
                if let Err(e) = open::that(url_str) {
                    eprintln!("Failed to open URL: {}", e);
                }
            }

            // Call completion handler
            unsafe {
                let block_ptr = completion_handler as *const _ as *const u8;
                let invoke_ptr: extern "C" fn(*const u8) =
                    *(block_ptr.add(16) as *const extern "C" fn(*const u8));
                invoke_ptr(block_ptr);
            }
        }
    }
);

impl NotificationDelegate {
    fn new() -> Id<Self> {
        unsafe { msg_send_id![Self::class(), new] }
    }
}

// Static delegate to prevent it from being dropped
fn get_delegate() -> &'static Id<NotificationDelegate> {
    static mut DELEGATE: MaybeUninit<Id<NotificationDelegate>> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();

    ONCE.call_once(|| unsafe {
        ptr::write(
            ptr::addr_of_mut!(DELEGATE),
            MaybeUninit::new(NotificationDelegate::new()),
        );
    });

    unsafe { (*ptr::addr_of!(DELEGATE)).assume_init_ref() }
}

pub fn startup() {
    // Get the notification center
    let center: *mut AnyObject = unsafe { msg_send![class!(UNUserNotificationCenter), currentNotificationCenter] };

    // Set up our delegate (must be static to avoid being dropped)
    let delegate = get_delegate();
    unsafe {
        let _: () = msg_send![center, setDelegate: delegate.as_ref()];
    }

    // Request authorization
    unsafe {
        let options = 7u64; // UNAuthorizationOptionBadge | UNAuthorizationOptionSound | UNAuthorizationOptionAlert
        let completion_block = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            if !granted.as_bool() {
                if !error.is_null() {
                    let error_ref = &*error;
                    let error_desc = error_ref.localizedDescription();
                    eprintln!("✗ Notification authorization denied: {}", error_desc);
                } else {
                    eprintln!("✗ Notification authorization denied - please enable in System Settings > Notifications");
                }
            }
        });

        let _: () = msg_send![
            center,
            requestAuthorizationWithOptions: options
            completionHandler: &*completion_block
        ];
    }

    // Create notification action for "Join" button
    unsafe {
        let action_id = NSString::from_str("JOIN_ACTION");
        let action_title = NSString::from_str("Join");
        let join_action: *mut AnyObject = msg_send![
            class!(UNNotificationAction),
            actionWithIdentifier: &*action_id
            title: &*action_title
            options: 1u64  // UNNotificationActionOptionForeground
        ];

        // Create notification category with the action
        let category_id = NSString::from_str("MEETING_CATEGORY");
        let actions_array: *mut AnyObject = {
            let array: *mut AnyObject = msg_send![class!(NSMutableArray), array];
            let _: () = msg_send![array, addObject: join_action];
            array
        };

        let category: *mut AnyObject = {
            let empty_array: *mut AnyObject = msg_send![class!(NSArray), array];
            msg_send![
                class!(UNNotificationCategory),
                categoryWithIdentifier: &*category_id
                actions: actions_array
                intentIdentifiers: empty_array
                options: 0u64
            ]
        };

        // Set the category on the notification center
        let categories_set: *mut AnyObject = msg_send![class!(NSSet), setWithObject: category];
        let _: () = msg_send![center, setNotificationCategories: categories_set];
    }
}

pub fn send(title: &str, subtitle: Option<&str>, body: &str, url: Option<&str>) {
    // Get the notification center
    let center: *mut AnyObject = unsafe { msg_send![class!(UNUserNotificationCenter), currentNotificationCenter] };

    // Create notification content
    let content: *mut AnyObject = unsafe { msg_send![class!(UNMutableNotificationContent), new] };
    unsafe {
        let title_ns = NSString::from_str(title);
        let body_ns = NSString::from_str(body);

        let _: () = msg_send![content, setTitle: &*title_ns];
        let _: () = msg_send![content, setBody: &*body_ns];

        // Set subtitle if provided
        if let Some(subtitle_str) = subtitle {
            let subtitle_ns = NSString::from_str(subtitle_str);
            let _: () = msg_send![content, setSubtitle: &*subtitle_ns];
        }

        // Set "Blow" sound
        let sound_name = NSString::from_str("Blow.aiff");
        let blow_sound: *mut AnyObject = msg_send![class!(UNNotificationSound), soundNamed: &*sound_name];
        let _: () = msg_send![content, setSound: blow_sound];

        // Set interruption level to active to ensure it makes sound
        let _: () = msg_send![content, setInterruptionLevel: 1u64]; // UNNotificationInterruptionLevelActive

        // Only set category and URL if url is provided
        if let Some(url_str) = url {
            // Set the category to show the Join button
            let category_id = NSString::from_str("MEETING_CATEGORY");
            let _: () = msg_send![content, setCategoryIdentifier: &*category_id];

            // Store the URL in userInfo dictionary
            let user_info_dict: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
            let url_key = NSString::from_str("url");
            let url_value = NSString::from_str(url_str);
            let _: () = msg_send![user_info_dict, setObject: &*url_value forKey: &*url_key];
            let _: () = msg_send![content, setUserInfo: user_info_dict];
        }
    }

    // Create notification request with a unique identifier
    let identifier = NSString::from_str(&format!(
        "nextcall-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let request: *mut AnyObject = unsafe {
        msg_send![
            class!(UNNotificationRequest),
            requestWithIdentifier: &*identifier
            content: content
            trigger: ptr::null::<AnyObject>()
        ]
    };

    // Add the notification request to the center
    let completion_block = RcBlock::new(move |error: *mut NSError| {
        if !error.is_null() {
            let error_ref = unsafe { &*error };
            let error_desc = error_ref.localizedDescription();
            eprintln!("Error scheduling notification: {}", error_desc);
        }
    });

    unsafe {
        let _: () = msg_send!(
            center,
            addNotificationRequest: request
            withCompletionHandler: &*completion_block
        );
    }
}
