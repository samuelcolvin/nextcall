# Calling Objective-C from Rust

How to build a macOS app whose OS-specific code is Objective-C and whose
business logic is Rust. Distilled from a working demo (`rust-objc/` — a Rust
binary that posts a system notification via ObjC); everything here is
project-portable.

## The core idea: bind through C, not through Objective-C

Objective-C is a strict superset of C, and Rust speaks the C ABI natively.
So the binding layer is never "Rust ↔ ObjC" — it is a small, deliberately
boring C API that both sides implement against:

```
Rust  ──extern "C"──►  C function  ◄──implemented in──  .m file
```

No bridging framework is required in either direction. ObjC objects,
blocks, and selectors never cross the boundary; only C types do (pointers,
integers, UTF-8 `char *`, function pointers).

There are three patterns, by who owns `main`:

| Pattern | Who calls whom | Use when |
|---|---|---|
| **A. Rust app, ObjC helpers** | Rust → ObjC | CLI-ish tools, Rust owns the run loop / lifecycle |
| **B. ObjC app, Rust core** | ObjC → Rust | Real GUI apps: AppKit shell, Rust logic (usually what you want) |
| **C. Pure Rust via `objc2`** | Rust → ObjC runtime | No `.m` files at all; see "When to use objc2 instead" |

Patterns A and B are the same boundary in opposite directions and compose
freely (callbacks make A ⊂ B anyway).

## Pattern A: Rust binary with ObjC source files

The `cc` crate compiles `.m` files into the cargo build. No Xcode project,
no Makefile needed for the compile itself.

`Cargo.toml`:

```toml
[package]
name = "myapp"
edition = "2024"

[build-dependencies]
cc = "1"
```

`build.rs`:

```rust
fn main() {
    println!("cargo:rerun-if-changed=src/native.m");
    cc::Build::new()
        .file("src/native.m")
        .flag("-fobjc-arc")
        .compile("native");
    // rustc drives the final link, so it won't add these automatically
    // the way clang does when it links .m files itself:
    println!("cargo:rustc-link-lib=objc");
    println!("cargo:rustc-link-lib=framework=Foundation");
    // add AppKit/UserNotifications/etc. as needed
}
```

`src/native.m` — expose C functions, keep ObjC internal:

```objc
#import <Foundation/Foundation.h>

void show_notification(const char *title, const char *body) {
    @autoreleasepool {
        // ... NSUserNotification / UNUserNotificationCenter etc.
    }
}
```

`src/main.rs`:

```rust
use std::ffi::{CString, c_char};

// edition 2024 requires `unsafe extern`
unsafe extern "C" {
    fn show_notification(title: *const c_char, body: *const c_char);
}

fn main() {
    let t = CString::new("Title").unwrap();
    let b = CString::new("Body").unwrap();
    unsafe { show_notification(t.as_ptr(), b.as_ptr()) };
}
```

`cargo build` / `cargo run` and that's the whole toolchain.

## Pattern B: ObjC app linking a Rust static library

For a real GUI app, invert it: AppKit owns `main`, Rust is a library.

**Workspace layout** — quarantine the FFI so the core stays a normal crate:

```
myapp/
├── Cargo.toml            # workspace
├── crates/
│   ├── app-core/         # pure Rust: logic, API calls, cargo test — no unsafe
│   └── app-ffi/          # the C boundary only; depends on app-core
└── macos/                # AppDelegate.m, views, Info.plist, Makefile/Xcode
```

`app-ffi/Cargo.toml`:

```toml
[lib]
crate-type = ["staticlib"]   # produces libapp_ffi.a
```

**Expose Rust objects as opaque handles** with explicit create/free:

```rust
pub struct AppCore { /* tokio runtime, http client, state */ }

#[unsafe(no_mangle)]
pub extern "C" fn app_core_new() -> *mut AppCore {
    Box::into_raw(Box::new(AppCore::new()))
}

#[unsafe(no_mangle)]
pub extern "C" fn app_core_free(core: *mut AppCore) {
    if !core.is_null() { drop(unsafe { Box::from_raw(core) }); }
}
```

**Generate the header with [cbindgen]** (`cbindgen crates/app-ffi -o
macos/src/app_core.h`, from a Makefile step or `build.rs`); the `.m` files
`#import` it. With only a handful of functions, a hand-written header is
also fine.

**Link it like any C library.** A Rust staticlib links cleanly with clang:

```make
libapp_ffi.a:
	cargo build --release -p app-ffi

app: libapp_ffi.a
	clang macos/src/*.m target/release/libapp_ffi.a \
	    -framework Cocoa -o build/MyApp.app/Contents/MacOS/MyApp
```

Or in Xcode: a Run Script phase that calls `cargo build`, plus the `.a` in
"Link Binary With Libraries". For universal binaries, build
`--target aarch64-apple-darwin` and `--target x86_64-apple-darwin`, then
`lipo -create` the two `.a`s.

[cbindgen]: https://github.com/mozilla/cbindgen

## Boundary design rules

These are where mixed projects rot; be strict about all of them.

- **C types only.** `*const c_char`, integers, `*mut c_void`, function
  pointers, opaque struct pointers. Never `NSString *`, never a block,
  never a Rust `String`/`Vec` by value.
- **Strings**: UTF-8 C strings both ways. ObjC side: `@(cstr)` in,
  `nsstring.UTF8String` out (valid only for the duration of the call —
  copy if kept). Rust-allocated strings returned to ObjC need a matching
  `xxx_string_free`; never `free()` Rust memory or `drop` C memory.
- **Callbacks**: C function pointer + `void *ctx` pair. On the ObjC side,
  wrap a block as the context (`(__bridge_retained void *)[block copy]`,
  `__bridge_transfer` back out exactly once — that pairing is the
  refcount).
- **Threads**: Rust async work (tokio etc.) completes on worker threads;
  AppKit/UIKit must only be touched on the main thread. Put the hop in the
  ObjC wrapper — `dispatch_async(dispatch_get_main_queue(), ...)` — so
  Rust stays platform-clean.
- **Errors**: return them as data (out-param, `(result, error)` callback
  arguments, or a status enum). Rust panics must not unwind into C — it is
  UB. Either build the FFI crate with `panic = "abort"` or wrap entry
  points in `std::panic::catch_unwind`.
- **Memory across the boundary is manual on both sides.** ARC does not see
  Rust; Rust's ownership does not see ObjC. Every allocation that crosses
  needs an explicit owner and an explicit free function.
- **Keep the API coarse.** Every crossing costs marshaling code and an
  `unsafe` audit. Prefer a few chunky operations ("fetch items" returning
  JSON) over a chatty object graph; serialize structured data (serde_json)
  instead of hand-marshaling structs field-by-field, except on measured
  hot paths.

## macOS packaging gotchas

- **Some APIs silently require a `.app` bundle.** Notifications
  (`NSUserNotificationCenter defaultUserNotificationCenter` returns nil
  without a `CFBundleIdentifier`), and most TCC-gated things behave better
  bundled. A cargo-built binary is bare; wrap it: copy into
  `MyApp.app/Contents/MacOS/`, add `Info.plist`, `codesign --force --sign -`.
- **Ad-hoc signing changes the code hash every build**, which can
  invalidate TCC grants (Accessibility, notifications). Use a stable
  `CFBundleIdentifier`; `tccutil reset <Service> <bundle-id>` when testing.
- **Toolchain skew bites.** clang and the SDK must be a matched pair;
  `xcode-select -p` pointing at a stale Xcode while the CLT (or OS) has
  moved on produces baffling modulemap errors. Keep one current toolchain
  selected and avoid pinning `-isysroot`/`SDKROOT` unless truly forced.

## When to use `objc2` instead

The [`objc2`](https://docs.rs/objc2) ecosystem (with generated framework
crates like `objc2-foundation`, `objc2-app-kit`) lets Rust call ObjC APIs
directly — typed methods, `Retained<T>` for refcounting,
`MainThreadMarker` for thread safety — with no `.m` files. Choose it when
you don't want ObjC source in the project at all. Its weak spot is the
reverse direction: implementing delegates or subclassing (`define_class!`)
is verbose and macro-heavy. If you're already comfortable writing ObjC,
patterns A/B keep the platform code in the platform language and the
boundary trivial — usually the better trade. (The older `objc`/`cocoa`
crates are unmaintained; don't use them. UniFFI/swift-bridge generate
Swift, not ObjC.)

Plenty of C-based Apple APIs (CoreGraphics, CoreFoundation, the
Accessibility C API) need none of this — they're ordinary FFI with
existing `-sys`/wrapper crates.

## Checklist for a new project

1. `cargo new`, workspace with `app-core` (pure) + `app-ffi` (staticlib) —
   or single crate with `cc` in `build.rs` for pattern A.
2. Design the C API: opaque handles, create/free pairs, UTF-8 strings,
   callback+ctx for async. Write it down before writing either side.
3. cbindgen for the header (or hand-write while small).
4. One thin ObjC wrapper class over the raw C API; main-queue hops live
   there.
5. `panic = "abort"` (or `catch_unwind`) on the FFI crate.
6. Makefile or Xcode script phase: `cargo build` → link `.a` → bundle →
   ad-hoc sign.
