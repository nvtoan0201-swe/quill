//! Single instance: the first Quill listens on a Unix socket; later launches
//! (file managers, `quill://` links) hand their arguments over and exit.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type Inbox = Arc<Mutex<Vec<String>>>;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let user = std::env::var("USER").unwrap_or_default();
    dir.join(format!("quill-{user}.sock"))
}

/// Send arguments to a running instance. Returns false when none is listening.
pub fn forward(args: &[String]) -> bool {
    let Ok(mut s) = UnixStream::connect(socket_path()) else { return false };
    let mut msg = args.join("\n");
    msg.push_str("\n\n");
    s.write_all(msg.as_bytes()).is_ok()
}

/// Start listening; messages land in the returned inbox.
pub fn listen() -> Inbox {
    let inbox: Inbox = Default::default();
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    if let Ok(listener) = UnixListener::bind(&path) {
        let inbox = inbox.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut got = Vec::new();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if line.is_empty() {
                        break;
                    }
                    got.push(line);
                }
                if let Ok(mut q) = inbox.lock() {
                    // An empty message just asks the window to come forward.
                    q.extend(if got.is_empty() { vec![String::new()] } else { got });
                }
            }
        });
    }
    inbox
}

/// Make file arguments absolute so the receiving instance can resolve them.
pub fn normalize(args: Vec<String>) -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    args.into_iter()
        .map(|a| {
            if a.starts_with("quill://") || a.starts_with('/') {
                a
            } else {
                cwd.join(a).to_string_lossy().into_owned()
            }
        })
        .collect()
}
