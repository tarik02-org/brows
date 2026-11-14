use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
use objc2::{AnyThread, ClassType, MainThreadMarker, class, msg_send, sel};
use std::collections::HashSet;
use std::ffi::CStr;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSEvent, NSScreen, NSWorkspace};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSNumber, NSPoint, NSRect, NSSearchPathDirectory,
    NSSearchPathDomainMask, NSSearchPathForDirectoriesInDomains, NSSize, NSString,
};
use tracing::{debug, warn};

use crate::MessageToMain;

/// Receives the get-URL Apple Events that open links with brows and forwards each link with
/// the bundle id of the app that sent it. Rune's `on_open_url` hook runs after the event is
/// dispatched, when the sender can no longer be read. Install it on the main thread before the
/// event loop starts, so the link that launched brows arrives here too; Rune leaves a handler
/// installed before its loop alone.
pub fn install_open_url_handler(main_sender: Sender<MessageToMain>) {
    const NAME: &CStr = c"BrowsOpenUrlHandler";

    if MainThreadMarker::new().is_none() {
        warn!("Could not install the open-URL handler off the main thread");
        return;
    }
    if OPEN_URL_SENDER.set(main_sender).is_err() {
        return;
    }

    let class = AnyClass::get(NAME).or_else(|| {
        let mut builder = ClassBuilder::new(NAME, NSObject::class())?;
        // SAFETY: `handle_get_url_event` matches `handleGetURLEvent:withReplyEvent:`.
        unsafe {
            builder.add_method(
                sel!(handleGetURLEvent:withReplyEvent:),
                handle_get_url_event as extern "C" fn(_, _, _, _),
            );
        }
        Some(builder.register())
    });
    let Some(class) = class else {
        warn!("Could not register {NAME:?}");
        return;
    };

    // SAFETY: Foundation calls on the main thread. The event manager doesn't retain its
    // handler, so the handler is never released.
    unsafe {
        let handler: *mut AnyObject = msg_send![class, new];
        let manager: *mut AnyObject =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let Some(manager) = manager.as_ref() else {
            warn!("Could not reach NSAppleEventManager");
            return;
        };
        let _: () = msg_send![
            manager,
            setEventHandler: handler,
            andSelector: sel!(handleGetURLEvent:withReplyEvent:),
            forEventClass: GET_URL_EVENT,
            andEventID: GET_URL_EVENT
        ];
    }
}

/// `'GURL'`: `kInternetEventClass` and `kAEGetURL`.
const GET_URL_EVENT: u32 = u32::from_be_bytes(*b"GURL");

static OPEN_URL_SENDER: OnceLock<Sender<MessageToMain>> = OnceLock::new();

extern "C" fn handle_get_url_event(
    _: &AnyObject,
    _: Sel,
    event: *mut AnyObject,
    _reply: *mut AnyObject,
) {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: AppKit passes the event being dispatched, or nil.
        let Some(event) = (unsafe { event.as_ref() }) else {
            return;
        };
        let Some(url) = apple_event_url(event) else {
            warn!("Received a get-URL event without a URL");
            return;
        };
        let source_bundle_id = apple_event_sender_bundle_id(event).unwrap_or_default();
        if let Some(sender) = OPEN_URL_SENDER.get() {
            sender
                .send(MessageToMain::UrlOpenRequest(source_bundle_id, url, None))
                .ok();
        }
    }));
    if caught.is_err() {
        warn!("Open-URL handler panicked");
    }
}

fn apple_event_url(event: &AnyObject) -> Option<String> {
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    // SAFETY: `event` is an NSAppleEventDescriptor; both messages are nil-safe.
    autoreleasepool(|_| unsafe {
        let param: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
        let url: *mut NSString = msg_send![param.as_ref()?, stringValue];
        Some(url.as_ref()?.to_string())
    })
}

fn apple_event_sender_bundle_id(event: &AnyObject) -> Option<String> {
    const KEY_ADDRESS_ATTR: u32 = u32::from_be_bytes(*b"addr");
    const TYPE_APPLICATION_BUNDLE_ID: u32 = u32::from_be_bytes(*b"bund");

    // SAFETY: `event` is an NSAppleEventDescriptor; every message is nil-safe.
    autoreleasepool(|_| unsafe {
        let sender: *mut AnyObject =
            msg_send![event, attributeDescriptorForKeyword: KEY_ADDRESS_ATTR];
        let bundle_id: *mut AnyObject =
            msg_send![sender.as_ref()?, coerceToDescriptorType: TYPE_APPLICATION_BUNDLE_ID];
        let bundle_id: *mut NSString = msg_send![bundle_id.as_ref()?, stringValue];
        Some(bundle_id.as_ref()?.to_string()).filter(|bundle_id| !bundle_id.is_empty())
    })
}

pub fn create_icon_for_app(full_path: &NSString, icon_path: &str) {
    unsafe {
        let shared_workspace = NSWorkspace::sharedWorkspace();

        let size = NSSize::new(64.0, 64.0);

        // NSImage
        let icon = shared_workspace.iconForFile(full_path);
        // resize to smaller
        icon.setSize(size);

        icon.lockFocus();

        let tiff = icon.TIFFRepresentation().unwrap();

        let rect = NSRect::new(NSPoint::new(0.0, 0.0), size);
        let rep_from_tiff = NSBitmapImageRep::imageRepWithData(&tiff).unwrap();

        // draws icon into the rectangle
        rep_from_tiff.drawInRect(rect);

        let rect_as_image =
            NSBitmapImageRep::initWithFocusedViewRect(NSBitmapImageRep::alloc(), rect).unwrap();
        icon.unlockFocus();

        let icon_png = rect_as_image
            .representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
            .unwrap();

        icon_png.writeToFile_atomically(&NSString::from_str(icon_path), true);
    }
}

pub fn get_bundle_url(bundle_id: &str) -> Option<Retained<NSString>> {
    debug!("Getting url for bundle: {}", bundle_id);

    unsafe {
        let shared_workspace = NSWorkspace::sharedWorkspace();

        // The URL of the app, or None if no app has the bundle identifier.
        shared_workspace
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
            .and_then(|url| url.relativePath())
    }
}

/// get macOS application support directory, supports sandboxing
pub fn macos_get_application_support_dir_path() -> PathBuf {
    macos_get_directory(NSSearchPathDirectory::ApplicationSupportDirectory)
}

/// get macOS caches directory, supports sandboxing
pub fn macos_get_caches_dir() -> PathBuf {
    macos_get_directory(NSSearchPathDirectory::CachesDirectory)
}

/// get macOS library directory, supports sandboxing
pub fn macos_get_library_dir() -> PathBuf {
    // LibraryDirectory is potentially sandboxed
    macos_get_directory(NSSearchPathDirectory::LibraryDirectory)
}

/// get macOS standard directory, supports sandboxing
pub fn macos_get_directory(directory: NSSearchPathDirectory) -> PathBuf {
    let results = unsafe {
        NSSearchPathForDirectoriesInDomains(directory, NSSearchPathDomainMask::UserDomainMask, true)
    };

    //let results = unsafe { CFArray::<CFString>::wrap_under_get_rule(results) };

    let option = results.firstObject();
    if option.is_none() {
        panic!("no")
    }

    let x = option.unwrap().to_string();

    PathBuf::from(x)
}

pub(crate) fn get_app_name(bundle_path: &NSString) -> String {
    let bundle = get_bundle(bundle_path);
    //bundleWithURL
    let bundle_name = bundle.name().unwrap();
    bundle_name.to_string()
}

pub(crate) fn get_app_executable_path(bundle_path: &NSString) -> String {
    let bundle = get_bundle(bundle_path);

    //bundleWithURL
    unsafe {
        let executable_path = bundle.executablePath().unwrap();
        executable_path.to_string()
    }
}

// returns NSBundle
fn get_bundle(bundle_path: &NSString) -> Retained<NSBundle> {
    //bundleWithURL
    unsafe {
        let bundle = NSBundle::bundleWithPath(bundle_path).unwrap();
        bundle
    }
}

// check schemes from an apps Info.plist CFBundleUrlTypes.CFBundleURLSchemes
pub fn get_bundle_ids_for_url_scheme(scheme: &str) -> Vec<String> {
    let scheme = NSString::from_str(scheme);

    let mut scheme_handlers = unsafe {
        // https scheme has some apps which are not browsers, e.g iterm2, Folx
        let scheme_handlers = LSCopyAllHandlersForURLScheme(&scheme);

        if scheme_handlers.is_none() {
            return Vec::new();
        }

        scheme_handlers
            .unwrap()
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>()
    };

    scheme_handlers.sort();

    let app_ids = scheme_handlers
        .iter()
        .map(|h| String::from(h.to_string()))
        .collect::<HashSet<_>>();

    Vec::from_iter(app_ids)
}

pub fn set_default_web_browser() -> bool {
    let bundle_id = "me.tarik02.brows";
    let bundle_id = NSString::from_str(bundle_id);
    let bundle_id = bundle_id.deref();

    let https_scheme = NSString::from_str("https");
    let https_scheme = https_scheme.deref();

    let http_scheme = NSString::from_str("http");
    let http_scheme = http_scheme.deref();

    unsafe {
        LSSetDefaultHandlerForURLScheme(https_scheme, bundle_id);
        LSSetDefaultHandlerForURLScheme(http_scheme, bundle_id);
    }

    return false;
}

pub fn is_default_web_browser() -> bool {
    let bundle_id = "me.tarik02.brows";
    //let bundle_id = NSString::from_str(bundle_id);
    //let bundle_id = bundle_id.deref();

    let https_scheme = NSString::from_str("https");
    let https_scheme = https_scheme.deref();

    let http_scheme = NSString::from_str("http");
    let http_scheme = http_scheme.deref();

    let https_bundle = unsafe { LSCopyDefaultHandlerForURLScheme(https_scheme) };
    let https_bundle = https_bundle.to_string();

    let http_bundle = unsafe { LSCopyDefaultHandlerForURLScheme(http_scheme) };
    let http_bundle = http_bundle.to_string();

    return https_bundle == bundle_id && http_bundle == bundle_id;
}

fn has_sandbox_entitlement2(bundle_url: &NSString) -> bool {
    unsafe {
        let is_sandboxed = false;
        //SecStaticCodeCreateWithPath(bundle_url, 0, nil)
        //CFUrlRef *bundleURL = [[NSBundle mainBundle] bundleURL];

        // Can use https://stackoverflow.com/a/42244464/752697
        /*
        BOOL isSandboxed = NO;

        SecStaticCodeRef staticCode = NULL;
        NSURL *bundleURL = [[NSBundle mainBundle] bundleURL];

        if (SecStaticCodeCreateWithPath((__bridge CFURLRef)bundleURL, kSecCSDefaultFlags, &staticCode) == errSecSuccess) {
            if (SecStaticCodeCheckValidityWithErrors(staticCode, kSecCSBasicValidateOnly, NULL, NULL) == errSecSuccess) {
                SecRequirementRef sandboxRequirement;
                if (SecRequirementCreateWithString(CFSTR("entitlement[\"com.apple.security.app-sandbox\"] exists"), kSecCSDefaultFlags,
                                               &sandboxRequirement) == errSecSuccess)
                {
                    OSStatus codeCheckResult = SecStaticCodeCheckValidityWithErrors(staticCode, kSecCSBasicValidateOnly, sandboxRequirement, NULL);
                    if (codeCheckResult == errSecSuccess) {
                        isSandboxed = YES;
                    }
                }
            }
            CFRelease(staticCode);
        }
        */
    }

    return false;

    // Or use codesign utility:
    // codesign - d - -entitlements - --xml "/Applications/Slack.app"

    // TODO: check if "com.apple.security.app-sandbox" key exists and if it's value is true
    /*
    Executable=/Applications/Slack.app/Contents/MacOS/Slack
    <?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
        "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
        <dict>
            <key>com.apple.security.app-sandbox</key>
            <true/>
            <key>com.apple.security.application-groups</key>
            <array>
                <string>BQR82RBBHL.com.tinyspeck.slackmacgap</string>
                <string>BQR82RBBHL.slack</string>
            </array>
            <key>com.apple.security.device.camera</key>
            <true/>
            <key>com.apple.security.device.microphone</key>
            <true/>
            <key>com.apple.security.device.usb</key>
            <true/>
            <key>com.apple.security.files.bookmarks.app-scope</key>
            <true/>
            <key>com.apple.security.files.downloads.read-write</key>
            <true/>
            <key>com.apple.security.files.user-selected.read-write</key>
            <true/>
            <key>com.apple.security.network.client</key>
            <true/>
            <key>com.apple.security.network.server</key>
            <true/>
            <key>com.apple.security.print</key>
            <true/>
        </dict>
    </plist>
     */
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSSetDefaultHandlerForURLScheme(scheme: &NSString, bundle_id: &NSString);

    // returns bundle id
    fn LSCopyDefaultHandlerForURLScheme(scheme: &NSString) -> Retained<NSString>;
}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    pub fn LSCopyAllHandlersForURLScheme(
        in_url_scheme: &NSString,
    ) -> Option<Retained<NSArray<NSString>>>;
}
