// System sleep tracking, exposed to Rust as power_start / power_asleep
// (src/power.rs).
//
// macOS runs processes during "dark wakes" (Power Nap, maintenance wakes)
// with the lid closed, so timers fire and speech would play to an empty room
// (or be heard hours later). NSWorkspace posts WillSleep before sleeping but
// DidWake only on a full user wake, never a dark wake, so a flag set by one and
// cleared by the other stays set throughout dark wakes.
#import <AppKit/AppKit.h>
#import <stdatomic.h>
#import <stdbool.h>

// True between WillSleep and DidWake. Atomic: Rust reads it off-main-thread.
static atomic_bool gAsleep = false;

// Registers the sleep/wake observers. Call once at startup, from any thread;
// blocks are delivered on the main queue, so they need tray_run's run loop.
void power_start(void) {
  NSNotificationCenter *center =
      [[NSWorkspace sharedWorkspace] notificationCenter];
  NSOperationQueue *main = [NSOperationQueue mainQueue];
  [center addObserverForName:NSWorkspaceWillSleepNotification
                      object:nil
                       queue:main
                  usingBlock:^(NSNotification *_) {
                    atomic_store(&gAsleep, true);
                  }];
  [center addObserverForName:NSWorkspaceDidWakeNotification
                      object:nil
                       queue:main
                  usingBlock:^(NSNotification *_) {
                    atomic_store(&gAsleep, false);
                  }];
}

// Whether the system is asleep (including dark wakes) as of the last
// notification. False before power_start.
bool power_asleep(void) { return atomic_load(&gAsleep); }
