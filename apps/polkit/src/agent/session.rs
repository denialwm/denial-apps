//! Small FFI boundary to Polkit's supported conversation API. No PAM implementation.
use super::Event;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_ulong, c_void},
    ptr,
    sync::mpsc::Sender,
};
use zeroize::Zeroizing;

type Object = *mut c_void;
type Callback = unsafe extern "C" fn();

#[link(name = "polkit-agent-1")]
unsafe extern "C" {
    fn polkit_agent_session_new(identity: Object, cookie: *const c_char) -> Object;
    fn polkit_agent_session_initiate(session: Object);
    fn polkit_agent_session_response(session: Object, response: *const c_char);
    fn polkit_agent_session_cancel(session: Object);
}
#[link(name = "polkit-gobject-1")]
unsafe extern "C" {
    fn polkit_unix_user_new(uid: c_int) -> Object;
    fn polkit_unix_user_get_name(identity: Object) -> *const c_char;
}
#[link(name = "gobject-2.0")]
unsafe extern "C" {
    fn g_object_unref(object: Object);
    fn g_signal_connect_data(
        object: Object,
        signal: *const c_char,
        callback: Callback,
        data: Object,
        destroy: Option<unsafe extern "C" fn(Object, Object)>,
        flags: c_int,
    ) -> c_ulong;
    fn g_signal_handler_disconnect(object: Object, id: c_ulong);
}
#[link(name = "glib-2.0")]
unsafe extern "C" {
    fn g_main_context_iteration(context: Object, may_block: c_int) -> c_int;
}

#[derive(Clone)]
pub enum Conversation {
    Prompt { text: String, echo: bool },
    Info(String),
    Error(String),
    Completed(bool),
}

struct Context {
    id: u64,
    attempt: u64,
    events: Sender<Event>,
}
impl Context {
    fn send(&self, value: Conversation) {
        let _ = self
            .events
            .send(Event::Conversation(self.id, self.attempt, value));
    }
}

// Callbacks only copy signal data into our queue. They never borrow the controller,
// invoke Flutter, or destroy a session while libpolkit is emitting a signal.
unsafe extern "C" fn prompt(_: Object, text: *const c_char, echo: c_int, data: Object) {
    // SAFETY: libpolkit supplies a valid UTF-8 C string and our boxed Context.
    unsafe {
        (&*data.cast::<Context>()).send(Conversation::Prompt {
            text: CStr::from_ptr(text).to_string_lossy().into_owned(),
            echo: echo != 0,
        });
    }
}
unsafe extern "C" fn info(_: Object, text: *const c_char, data: Object) {
    // SAFETY: signal ABI matches show-info, and Context lives until disconnection.
    unsafe {
        (&*data.cast::<Context>()).send(Conversation::Info(
            CStr::from_ptr(text).to_string_lossy().into_owned(),
        ));
    }
}
unsafe extern "C" fn error(_: Object, text: *const c_char, data: Object) {
    // SAFETY: signal ABI matches show-error, and Context lives until disconnection.
    unsafe {
        (&*data.cast::<Context>()).send(Conversation::Error(
            CStr::from_ptr(text).to_string_lossy().into_owned(),
        ));
    }
}
unsafe extern "C" fn completed(_: Object, gained: c_int, data: Object) {
    // SAFETY: signal ABI matches completed, and Context lives until disconnection.
    unsafe {
        (&*data.cast::<Context>()).send(Conversation::Completed(gained != 0));
    }
}

pub fn identity_name(uid: u32) -> String {
    // SAFETY: uid was validated as a signed gint; identity owns one GObject reference.
    unsafe {
        let identity = polkit_unix_user_new(uid as c_int);
        let name = polkit_unix_user_get_name(identity);
        let result = if name.is_null() {
            uid.to_string()
        } else {
            CStr::from_ptr(name).to_string_lossy().into_owned()
        };
        g_object_unref(identity);
        result
    }
}

pub fn dispatch() {
    // SAFETY: all sessions and GLib callbacks are confined to this main thread.
    unsafe { while g_main_context_iteration(ptr::null_mut(), 0) != 0 {} }
}

pub struct Session {
    object: Object,
    handlers: Vec<c_ulong>,
    // The stable allocation is referenced by all signal handlers.
    _context: Box<Context>,
    finished: bool,
}

impl Session {
    pub fn new(
        uid: u32,
        cookie: &str,
        id: u64,
        attempt: u64,
        events: Sender<Event>,
    ) -> Result<Self, String> {
        let cookie = CString::new(cookie).map_err(|_| "Invalid authentication cookie")?;
        let mut context = Box::new(Context {
            id,
            attempt,
            events,
        });
        let data = (&mut *context as *mut Context).cast();
        // SAFETY: identity and cookie are valid for construction. Polkit copies
        // the cookie and references the identity. Callback signatures match GIR.
        unsafe {
            let identity = polkit_unix_user_new(uid as c_int);
            let object = polkit_agent_session_new(identity, cookie.as_ptr());
            g_object_unref(identity);
            if object.is_null() {
                return Err("Could not create Polkit conversation".into());
            }
            let signals = [
                (
                    c"request",
                    std::mem::transmute::<
                        unsafe extern "C" fn(Object, *const c_char, c_int, Object),
                        Callback,
                    >(prompt),
                ),
                (
                    c"show-info",
                    std::mem::transmute::<
                        unsafe extern "C" fn(Object, *const c_char, Object),
                        Callback,
                    >(info),
                ),
                (
                    c"show-error",
                    std::mem::transmute::<
                        unsafe extern "C" fn(Object, *const c_char, Object),
                        Callback,
                    >(error),
                ),
                (
                    c"completed",
                    std::mem::transmute::<unsafe extern "C" fn(Object, c_int, Object), Callback>(
                        completed,
                    ),
                ),
            ];
            let handlers = signals
                .into_iter()
                .map(|(name, callback)| {
                    g_signal_connect_data(object, name.as_ptr(), callback, data, None, 0)
                })
                .collect();
            let session = Self {
                object,
                handlers,
                _context: context,
                finished: false,
            };
            polkit_agent_session_initiate(object);
            Ok(session)
        }
    }

    pub fn respond(&self, response: Zeroizing<String>) -> Result<(), String> {
        // The helper protocol is line based; reject embedded line terminators/NUL.
        if response.contains(['\0', '\n', '\r']) {
            return Err("Response contains an unsupported character".into());
        }
        let mut bytes = Zeroizing::new(response.as_bytes().to_vec());
        bytes.push(0);
        // SAFETY: this session remains alive; the response is terminated and copied
        // by libpolkit synchronously. Both Rust buffers are wiped on return.
        unsafe {
            polkit_agent_session_response(self.object, bytes.as_ptr().cast());
        }
        Ok(())
    }

    pub fn completed(&mut self) {
        self.finished = true;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: handlers belong to this owned GObject. Disconnect before cancel
        // so no callback can access Context during/after its destruction.
        unsafe {
            for id in self.handlers.drain(..) {
                g_signal_handler_disconnect(self.object, id);
            }
            if !self.finished {
                polkit_agent_session_cancel(self.object);
            }
            g_object_unref(self.object);
        }
    }
}
