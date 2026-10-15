//! Hosted Xvfb fixture: request the owned GTK window's normal close path, not WebKit closePage.
use std::{
    collections::VecDeque,
    ffi::{CString, c_char, c_int, c_long, c_uchar, c_ulong, c_void},
    path::Path,
    ptr,
};

type Window = c_ulong;
#[repr(C)]
struct ClientMessage {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: Window,
    message_type: c_ulong,
    format: c_int,
    data: [c_long; 5],
}
#[repr(C)]
union Event {
    client: std::mem::ManuallyDrop<ClientMessage>,
    pad: [c_long; 24],
}
#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut c_void;
    fn XCloseDisplay(display: *mut c_void) -> c_int;
    fn XDefaultRootWindow(display: *mut c_void) -> Window;
    fn XInternAtom(display: *mut c_void, name: *const c_char, only: c_int) -> c_ulong;
    fn XQueryTree(
        display: *mut c_void,
        window: Window,
        root: *mut Window,
        parent: *mut Window,
        children: *mut *mut Window,
        count: *mut u32,
    ) -> c_int;
    fn XGetWindowProperty(
        display: *mut c_void,
        window: Window,
        property: c_ulong,
        offset: c_long,
        length: c_long,
        delete: c_int,
        requested: c_ulong,
        actual: *mut c_ulong,
        format: *mut c_int,
        count: *mut c_ulong,
        remaining: *mut c_ulong,
        data: *mut *mut c_uchar,
    ) -> c_int;
    fn XGetWMProtocols(
        display: *mut c_void,
        window: Window,
        protocols: *mut *mut c_ulong,
        count: *mut c_int,
    ) -> c_int;
    fn XSendEvent(
        display: *mut c_void,
        window: Window,
        propagate: c_int,
        mask: c_long,
        event: *mut Event,
    ) -> c_int;
    fn XSync(display: *mut c_void, discard: c_int) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
}
struct Display(*mut c_void);
impl Drop for Display {
    fn drop(&mut self) {
        // SAFETY: this fixture exclusively owns the non-null display connection.
        unsafe {
            XCloseDisplay(self.0);
        }
    }
}
fn birth(pid: u32) -> Result<String, Box<dyn std::error::Error>> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    Ok(stat
        .rsplit_once(") ")
        .ok_or("Invalid process identity.")?
        .1
        .split_whitespace()
        .nth(19)
        .ok_or("Missing process birth time.")?
        .to_owned())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted") {
        return Err("Native close fixture is hosted-only.".into());
    }
    let display_name = std::env::var("DISPLAY")?;
    if !display_name.starts_with(':')
        || !display_name[1..]
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
    {
        return Err("An isolated local Xvfb display is required.".into());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("Expected owned PID, executable and birth ticks.".into());
    }
    let pid: u32 = args[0].parse()?;
    let executable = Path::new(&args[1]).canonicalize()?;
    let check = || -> Result<(), Box<dyn std::error::Error>> {
        if birth(pid)? != args[2] || std::fs::read_link(format!("/proc/{pid}/exe"))? != executable {
            return Err("Owned application identity changed.".into());
        }
        Ok(())
    };
    check()?;
    // SAFETY: Xlib owns returned buffers; each is checked and freed before the connection closes.
    unsafe {
        let raw = XOpenDisplay(ptr::null());
        if raw.is_null() {
            return Err("Isolated X display is unavailable.".into());
        }
        let display = Display(raw);
        let atom = |name: &str| XInternAtom(display.0, CString::new(name).unwrap().as_ptr(), 1);
        let pid_atom = atom("_NET_WM_PID");
        let protocols_atom = atom("WM_PROTOCOLS");
        let delete_atom = atom("WM_DELETE_WINDOW");
        if [pid_atom, protocols_atom, delete_atom].contains(&0) {
            return Err("Native window close protocol is unavailable.".into());
        }
        let mut queue = VecDeque::from([XDefaultRootWindow(display.0)]);
        let mut visited = 0;
        let mut matches = Vec::new();
        while let Some(window) = queue.pop_front() {
            visited += 1;
            if visited > 4096 {
                return Err("Native window discovery exceeded its bound.".into());
            }
            let (mut actual, mut format, mut count, mut remaining) = (0, 0, 0, 0);
            let mut data = ptr::null_mut();
            let result = XGetWindowProperty(
                display.0,
                window,
                pid_atom,
                0,
                1,
                0,
                6,
                &mut actual,
                &mut format,
                &mut count,
                &mut remaining,
                &mut data,
            );
            let owned = result == 0
                && actual == 6
                && format == 32
                && count == 1
                && !data.is_null()
                && *(data.cast::<c_ulong>()) == c_ulong::from(pid);
            if !data.is_null() {
                XFree(data.cast());
            }
            if owned {
                let mut protocols = ptr::null_mut();
                let mut size = 0;
                if XGetWMProtocols(display.0, window, &mut protocols, &mut size) != 0 {
                    if !protocols.is_null()
                        && (0..=128).contains(&size)
                        && std::slice::from_raw_parts(protocols, size as usize)
                            .contains(&delete_atom)
                    {
                        matches.push(window);
                    }
                    if !protocols.is_null() {
                        XFree(protocols.cast());
                    }
                }
            }
            let (mut root, mut parent, mut children, mut size) = (0, 0, ptr::null_mut(), 0);
            if XQueryTree(
                display.0,
                window,
                &mut root,
                &mut parent,
                &mut children,
                &mut size,
            ) != 0
            {
                if !children.is_null() {
                    if size as usize > 4096 - visited {
                        XFree(children.cast());
                        return Err("Native window fanout exceeded its bound.".into());
                    }
                    queue.extend(std::slice::from_raw_parts(children, size as usize));
                    XFree(children.cast());
                }
            }
        }
        if matches.len() != 1 {
            return Err("Expected exactly one closeable owned native window.".into());
        }
        check()?;
        let window = matches[0];
        let mut event = Event {
            client: std::mem::ManuallyDrop::new(ClientMessage {
                kind: 33,
                serial: 0,
                send_event: 1,
                display: display.0,
                window,
                message_type: protocols_atom,
                format: 32,
                data: [delete_atom as c_long, 0, 0, 0, 0],
            }),
        };
        if XSendEvent(display.0, window, 0, 0, &mut event) == 0 {
            return Err("Native close request could not be sent.".into());
        }
        XSync(display.0, 0);
    }
    eprintln!("Owned native WM_DELETE_WINDOW request sent.");
    Ok(())
}
