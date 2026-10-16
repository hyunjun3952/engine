// Asks the window manager to make a window fullscreen or ordinary again (minifb has no call for it).

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn set(window: *mut std::ffi::c_void, on: bool) -> Result<(), String> {
    use std::ffi::{c_long, c_ulong};
    use x11_dl::xlib::{self, Xlib};

    let xlib = Xlib::open().map_err(|e| format!("no Xlib: {e}"))?;
    // SAFETY: the calls follow the Xlib contract: the display is opened and closed here, the atoms are interned on it,
    // and the event is fully initialised before it is sent. `window` is the id minifb made on the same X server.
    unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return Err("cannot open the X display".into());
        }
        let atom = |name: &std::ffi::CStr| (xlib.XInternAtom)(display, name.as_ptr(), xlib::False);
        let mut message: xlib::XClientMessageEvent = std::mem::zeroed();
        message.type_ = xlib::ClientMessage;
        message.window = window as c_ulong;
        message.message_type = atom(c"_NET_WM_STATE");
        message.format = 32;
        message.data.set_long(0, on as c_long);
        message.data.set_long(1, atom(c"_NET_WM_STATE_FULLSCREEN") as c_long);
        message.data.set_long(3, 1);
        let mut event = xlib::XEvent { client_message: message };
        let root = (xlib.XDefaultRootWindow)(display);
        let mask = xlib::SubstructureNotifyMask | xlib::SubstructureRedirectMask;
        let sent = (xlib.XSendEvent)(display, root, xlib::False, mask, &mut event);
        (xlib.XFlush)(display);
        (xlib.XCloseDisplay)(display);
        if sent == 0 {
            return Err("the window manager did not take the request".into());
        }
    }
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub(crate) fn set(_window: *mut std::ffi::c_void, _on: bool) -> Result<(), String> {
    Err("fullscreen is not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use minifb::{Window, WindowOptions};

    // Needs a display: `cargo test -- --ignored fullscreen`.
    #[test]
    #[ignore = "needs a display"]
    fn a_real_window_grows_to_the_screen_and_comes_back() {
        let mut win = Window::new("probe", 320, 180, WindowOptions::default()).unwrap();
        let buf = vec![0x336699u32; 320 * 180];
        let mut sizes = Vec::new();
        for frame in 0..120 {
            win.update_with_buffer(&buf, 320, 180).unwrap();
            match frame {
                20 => super::set(win.get_window_handle(), true).unwrap(),
                70 => super::set(win.get_window_handle(), false).unwrap(),
                _ => {}
            }
            if frame == 60 || frame == 110 {
                sizes.push(win.get_size());
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        assert!(sizes[0].0 > 320 && sizes[0].1 > 180, "fullscreen: {sizes:?}");
        assert_eq!(sizes[1], (320, 180), "and back: {sizes:?}");
    }
}
