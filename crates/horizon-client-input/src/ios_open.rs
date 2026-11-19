//! iOS: opens a web page in the browser (`-[UIApplication openURL:options:completionHandler:]`).
#![allow(unsafe_code)]
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

/// Opens `url` (an `https:` address) in Safari; false when iOS can't.
pub fn open_url(url: &str) -> bool {
    let text = NSString::from_str(url);
    // SAFETY: documented UIKit and Foundation class methods, on the main thread (the game's).
    unsafe {
        let nsurl: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*text];
        let Some(nsurl) = nsurl.as_ref() else {
            return false;
        };
        let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
        let Some(app) = app.as_ref() else {
            return false;
        };
        let options: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let none: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![app, openURL: nsurl, options: options, completionHandler: none];
    }
    true
}
