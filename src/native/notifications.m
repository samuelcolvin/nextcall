// System notifications via the UserNotifications framework.
//
// Exposed to Rust as plain C functions (declared in src/notifications.rs);
// only UTF-8 C strings cross the boundary, all ObjC stays in this file.
// Note: UNUserNotificationCenter is unavailable outside a signed .app bundle
// with a CFBundleIdentifier - a bare cargo-built binary will crash here.
#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>

// Identifiers shared between category registration (startup) and send.
static NSString *const kMeetingCategory = @"MEETING_CATEGORY";
static NSString *const kJoinAction = @"JOIN_ACTION";

// Delegate that keeps banners visible while the app is frontmost and opens
// the meeting URL when the notification (or its Join button) is clicked.
@interface NCNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation NCNotificationDelegate

// Show banner, play sound and badge even if the app is considered active.
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completionHandler {
    completionHandler(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionSound |
                      UNNotificationPresentationOptionBadge);
}

// Opens the meeting URL in its default handler, explicitly handing focus over.
// Clicking the notification body activates nextcall — a windowless accessory
// app — which can win the activation race against the browser and leave its
// window frontmost but *inactive* (mouse clicks swallowed, hover broken).
// activates=YES plus cooperative yielding makes the handoff deterministic.
static void open_meeting_url(NSURL *nsurl) {
    NSWorkspace *ws = [NSWorkspace sharedWorkspace];
    // macOS 14+ cooperative activation: declare that the URL's handler may take
    // focus from us, otherwise its activation request can be silently refused.
    if (@available(macOS 14.0, *)) {
        NSURL *appURL = [ws URLForApplicationToOpenURL:nsurl];
        NSString *bundleID = appURL != nil ? [NSBundle bundleWithURL:appURL].bundleIdentifier : nil;
        if (bundleID != nil) {
            [NSApp yieldActivationToApplicationWithBundleIdentifier:bundleID];
        }
    }
    NSWorkspaceOpenConfiguration *config = [NSWorkspaceOpenConfiguration configuration];
    config.activates = YES;
    [ws openURL:nsurl configuration:config completionHandler:nil];
}

// Any interaction (banner click or Join button) opens the stored video link.
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
    didReceiveNotificationResponse:(UNNotificationResponse *)response
             withCompletionHandler:(void (^)(void))completionHandler {
    NSString *url = response.notification.request.content.userInfo[@"url"];
    if ([url isKindOfClass:[NSString class]]) {
        NSURL *nsurl = [NSURL URLWithString:url];
        if (nsurl != nil) {
            // Delegate callbacks arrive on a background queue; NSApp
            // (activation yielding) must be touched on the main thread.
            dispatch_async(dispatch_get_main_queue(), ^{
              open_meeting_url(nsurl);
            });
        }
    }
    completionHandler();
}

@end

// The center holds its delegate weakly, so this static strong reference is
// what keeps the delegate alive for the process lifetime.
static NCNotificationDelegate *gDelegate = nil;

// Installs the delegate, requests notification permission and registers the
// MEETING_CATEGORY with a "Join" action. Call once, before notifications_send.
void notifications_startup(void) {
    @autoreleasepool {
        UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];

        gDelegate = [[NCNotificationDelegate alloc] init];
        center.delegate = gDelegate;

        UNAuthorizationOptions options =
            UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge;
        [center requestAuthorizationWithOptions:options
                              completionHandler:^(BOOL granted, NSError *_Nullable error) {
                                if (!granted) {
                                    if (error != nil) {
                                        fprintf(stderr, "✗ Notification authorization denied: %s\n",
                                                error.localizedDescription.UTF8String);
                                    } else {
                                        fprintf(stderr, "✗ Notification authorization denied - please enable in "
                                                        "System Settings > Notifications\n");
                                    }
                                }
                              }];

        // Background action (options:0): Foreground would activate *nextcall*,
        // racing the browser's activation from open_meeting_url and leaving the
        // browser's window frontmost but not accepting mouse input properly.
        UNNotificationAction *join = [UNNotificationAction actionWithIdentifier:kJoinAction
                                                                          title:@"Join"
                                                                        options:0];
        UNNotificationCategory *category = [UNNotificationCategory categoryWithIdentifier:kMeetingCategory
                                                                                   actions:@[ join ]
                                                                         intentIdentifiers:@[]
                                                                                   options:0];
        [center setNotificationCategories:[NSSet setWithObject:category]];
    }
}

// Posts a notification immediately. subtitle and url may be NULL; a non-NULL
// url adds the Join button and makes any click open that link.
// Thread-safe: UNUserNotificationCenter may be called from any thread.
void notifications_send(const char *title, const char *subtitle, const char *body, const char *url) {
    @autoreleasepool {
        UNMutableNotificationContent *content = [[UNMutableNotificationContent alloc] init];
        content.title = @(title);
        content.body = @(body);
        if (subtitle != NULL) {
            content.subtitle = @(subtitle);
        }

        // "Blow" with active interruption level so the alert reliably makes sound.
        content.sound = [UNNotificationSound soundNamed:@"Blow.aiff"];
        content.interruptionLevel = UNNotificationInterruptionLevelActive;

        if (url != NULL) {
            content.categoryIdentifier = kMeetingCategory;
            // The delegate reads this back out when the notification is clicked.
            content.userInfo = @{@"url" : @(url)};
        }

        NSString *identifier = [NSString stringWithFormat:@"nextcall-%@", [[NSUUID UUID] UUIDString]];
        UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:identifier
                                                                              content:content
                                                                              trigger:nil];

        UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
        [center addNotificationRequest:request
                 withCompletionHandler:^(NSError *_Nullable error) {
                   if (error != nil) {
                       fprintf(stderr, "Error scheduling notification: %s\n",
                               error.localizedDescription.UTF8String);
                   }
                 }];
    }
}
