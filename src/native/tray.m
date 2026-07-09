// Menu bar status item and AppKit run loop.
//
// Replaces the old winit + tray-icon + rendered-PNG approach: NSStatusItem
// displays plain text natively, so the countdown is just a title string.
// Exposed to Rust as tray_run / tray_set_title (declared in src/tray.rs).
#import <AppKit/AppKit.h>

// This run's log file, set via tray_set_log_path; nil until then. Declared
// before NCMenuActions because its openLog: action reads it.
static NSString *gLogPath = nil;

// Target for menu items with custom actions. NSMenuItem holds its target
// weakly, so the static gMenuActions reference below keeps it alive.
@interface NCMenuActions : NSObject
@end

@implementation NCMenuActions

// Opens the project repository in the default browser.
- (void)openGitHub:(id)sender {
    [[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"https://github.com/samuelcolvin/nextcall"]];
}

// Opens this run's log file in the default .log viewer (same as `open <path>`).
// No-op until Rust has set the path via tray_set_log_path.
- (void)openLog:(id)sender {
    if (gLogPath != nil) {
        [[NSWorkspace sharedWorkspace] openURL:[NSURL fileURLWithPath:gLogPath]];
    }
}

@end

// Created once in tray_run on the main thread; live for the process lifetime.
static NSStatusItem *gStatusItem = nil;
static NCMenuActions *gMenuActions = nil;
// First menu entry: a disabled line showing the current status (next call /
// call in progress). NSMenu auto-disables it because it has no action.
static NSMenuItem *gStatusMenuItem = nil;

// Creates the status item with a Quit menu and runs the AppKit event loop.
// Must be called on the main thread; never returns ("Quit" terminates the
// process via NSApp terminate:).
void tray_run(void) {
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        // Accessory: menu bar presence only, no Dock icon or app menu.
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];

        gStatusItem = [[NSStatusBar systemStatusBar] statusItemWithLength:NSVariableStatusItemLength];
        gStatusItem.button.title = @"...";

        gMenuActions = [[NCMenuActions alloc] init];
        NSMenu *menu = [[NSMenu alloc] init];
        gStatusMenuItem = [[NSMenuItem alloc] initWithTitle:@"Loading calendar…" action:nil keyEquivalent:@""];
        [menu addItem:gStatusMenuItem];
        [menu addItem:[NSMenuItem separatorItem]];
        NSMenuItem *github = [[NSMenuItem alloc] initWithTitle:@"About nextcall"
                                                        action:@selector(openGitHub:)
                                                 keyEquivalent:@""];
        github.target = gMenuActions;
        [menu addItem:github];
        NSMenuItem *viewLog = [[NSMenuItem alloc] initWithTitle:@"View Log"
                                                         action:@selector(openLog:)
                                                  keyEquivalent:@""];
        viewLog.target = gMenuActions;
        [menu addItem:viewLog];
        [menu addItem:[NSMenuItem separatorItem]];
        // nil target: the responder chain routes terminate: to NSApp.
        [menu addItemWithTitle:@"Quit Nextcall" action:@selector(terminate:) keyEquivalent:@""];
        gStatusItem.menu = menu;

        [app run];
    }
}

// Updates the status item text. Thread-safe: hops to the main queue, so it
// may be called from Rust worker threads (and before tray_run - the update is
// applied once the loop starts).
void tray_set_title(const char *title) {
    @autoreleasepool {
        // Copy to NSString now: the char* is only valid for this call.
        NSString *text = @(title);
        dispatch_async(dispatch_get_main_queue(), ^{
          gStatusItem.button.title = text;
        });
    }
}

// Updates the status line at the top of the menu. Thread-safe, same
// main-queue rules as tray_set_title.
void tray_set_status(const char *status) {
    @autoreleasepool {
        NSString *text = @(status);
        dispatch_async(dispatch_get_main_queue(), ^{
          gStatusMenuItem.title = text;
        });
    }
}

// Records the path opened by the "View Log" menu item. Thread-safe, same
// main-queue rules as tray_set_title; called once from Rust at startup.
void tray_set_log_path(const char *path) {
    @autoreleasepool {
        NSString *text = @(path);
        dispatch_async(dispatch_get_main_queue(), ^{
          gLogPath = text;
        });
    }
}

