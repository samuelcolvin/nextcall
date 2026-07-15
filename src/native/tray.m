// Menu bar status item and AppKit run loop.
//
// Replaces the old winit + tray-icon + rendered-PNG approach: NSStatusItem
// displays plain text natively, so the countdown is just a title string. The
// only images are template glyphs: the logo while idle, bell.slash while
// dismissed. Exposed to Rust as tray_run / tray_set_title (src/tray.rs).
#import <AppKit/AppKit.h>
#import <stdatomic.h>
#import <stdbool.h>

// This run's log file, set via tray_set_log_path; nil until then. Declared
// before NCMenuActions because its openLog: action reads it.
static NSString *gLogPath = nil;

// The "Dismiss" toggle, owned by the tray: Rust arms gDismissTarget each tick
// (next call's start unix time; 0 = no call, item disabled) and a click flips
// gDismissedTs between 0 and the target. Atomics: Rust polls off-main-thread.
static _Atomic int64_t gDismissTarget = 0;
static _Atomic int64_t gDismissedTs = 0;

// Created once in tray_run on the main thread; live for the process lifetime.
static NSStatusItem *gStatusItem = nil;
// The "Dismiss" / "Revert dismiss" menu item; its title tracks gDismissedTs.
static NSMenuItem *gDismissMenuItem = nil;
// Last raw countdown text from Rust; render() derives the display from it.
static NSString *gTitle = @"...";
// The stopwatch-lens logo (assets/tray-icon.png in Resources), shown instead
// of the idle "..." text; nil outside a bundle, which falls back to "...".
static NSImage *gIdleIcon = nil;

// Renders the status item and Dismiss menu item from (gTitle, gDismissedTs) —
// the one place display state is applied, called when either input changes.
// Main thread only.
static void render(void) {
    bool dismissed = atomic_load(&gDismissedTs) != 0;
    bool idle = [gTitle isEqualToString:@"..."];
    if (dismissed) {
        // the muted bell joins the countdown, or alone replaces the idle "..."
        gStatusItem.button.title = idle ? @"" : gTitle;
        // SF Symbol = monochrome template image, follows menu bar light/dark
        gStatusItem.button.image = [NSImage imageWithSystemSymbolName:@"bell.slash"
                                             accessibilityDescription:@"alerts dismissed"];
        gStatusItem.button.imagePosition = NSImageLeft;
    } else if (idle && gIdleIcon != nil) {
        // no upcoming call: show the logo rather than "..."
        gStatusItem.button.title = @"";
        gStatusItem.button.image = gIdleIcon;
        gStatusItem.button.imagePosition = NSImageOnly;
    } else {
        gStatusItem.button.title = gTitle;
        gStatusItem.button.image = nil;
    }
    gDismissMenuItem.title = dismissed ? @"Revert dismiss" : @"Dismiss";
}

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

// Toggles the dismissed state for the armed call and re-renders immediately
// (no waiting on the Rust loop, which may be mid-sleep for minutes). Rust
// picks the new state up on its next tick, always before any alert fires.
- (void)dismissCall:(id)sender {
    int64_t target = atomic_load(&gDismissTarget);
    if (target == 0) {
        return;
    }
    atomic_store(&gDismissedTs, atomic_load(&gDismissedTs) == 0 ? target : 0);
    render();
}

// Greys out "Dismiss" when there is no upcoming call to act on (target 0).
- (BOOL)validateMenuItem:(NSMenuItem *)item {
    if (item.action == @selector(dismissCall:)) {
        return atomic_load(&gDismissTarget) != 0;
    }
    return YES;
}

@end

// Created once in tray_run on the main thread; live for the process lifetime.
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

        // Template image: menu bar recolors it for light/dark. 36px PNG shown
        // at 18pt, i.e. @2x. Nil-safe: property sets on nil are no-ops.
        gIdleIcon = [[NSBundle mainBundle] imageForResource:@"tray-icon"];
        gIdleIcon.size = NSMakeSize(18, 18);
        gIdleIcon.template = YES;
        render();

        gMenuActions = [[NCMenuActions alloc] init];
        NSMenu *menu = [[NSMenu alloc] init];
        gStatusMenuItem = [[NSMenuItem alloc] initWithTitle:@"Loading calendar…" action:nil keyEquivalent:@""];
        [menu addItem:gStatusMenuItem];
        gDismissMenuItem = [[NSMenuItem alloc] initWithTitle:@"Dismiss"
                                                      action:@selector(dismissCall:)
                                               keyEquivalent:@""];
        gDismissMenuItem.target = gMenuActions;
        [menu addItem:gDismissMenuItem];
        [menu addItem:[NSMenuItem separatorItem]];
        NSMenuItem *viewLog = [[NSMenuItem alloc] initWithTitle:@"View Log"
                                                         action:@selector(openLog:)
                                                  keyEquivalent:@""];
        viewLog.target = gMenuActions;
        [menu addItem:viewLog];
        NSMenuItem *github = [[NSMenuItem alloc] initWithTitle:@"About Nextcall"
                                                        action:@selector(openGitHub:)
                                                 keyEquivalent:@""];
        github.target = gMenuActions;
        [menu addItem:github];
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
          gTitle = text;
          render();
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

// Arms the "Dismiss" menu item with the current call's start unix time
// (0 = no call: the item is disabled), clearing a dismissal that no longer
// refers to it — how a dismissal expires when the next call changes. Called
// from Rust every tick.
void tray_set_dismiss_target(int64_t start_ts) {
    atomic_store(&gDismissTarget, start_ts);
    int64_t stale = atomic_load(&gDismissedTs);
    if (stale != 0 && stale != start_ts) {
        // CAS: only clear the stale value we saw, never a racing fresh click
        atomic_compare_exchange_strong(&gDismissedTs, &stale, 0);
    }
    dispatch_async(dispatch_get_main_queue(), ^{
      render();
    });
}

// The start unix time of the call the user dismissed via the menu (0 = none).
// Polled from Rust each tick and matched against the current next call there
// before suppressing alerts, so a stale value is harmless.
int64_t tray_dismissed_ts(void) {
    return atomic_load(&gDismissedTs);
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
