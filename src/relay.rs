//! A byte relay: accept on one socket, connect to a TCP address, copy both
//! ways. Two uses:
//! - the rootless engine: a guest's broker connection arrives on a unix
//!   socket and leaves for errand's broker on host 127.0.0.1 (E-23);
//! - image builds: the host's egress proxy listens on loopback only, so the
//!   build instance reaches it through a relay on the build bridge (E-10).

use std::io;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;

fn pump<A, B>(
    a: A,
    b: B,
    close_a: impl Fn(&A) + Send + 'static,
    close_b: impl Fn(&B) + Send + 'static,
) where
    A: io::Read + io::Write + Send + 'static + TryClone,
    B: io::Read + io::Write + Send + 'static + TryClone,
{
    let (Ok(a2), Ok(b2)) = (a.try_clone_box(), b.try_clone_box()) else {
        return;
    };
    thread::spawn(move || {
        let mut a = a;
        let mut b2 = b2;
        let _ = io::copy(&mut a, &mut b2);
        close_b(&b2);
    });
    thread::spawn(move || {
        let mut b = b;
        let mut a2 = a2;
        let _ = io::copy(&mut b, &mut a2);
        close_a(&a2);
    });
}

pub trait TryClone: Sized {
    fn try_clone_box(&self) -> io::Result<Self>;
}

impl TryClone for TcpStream {
    fn try_clone_box(&self) -> io::Result<Self> {
        self.try_clone()
    }
}

impl TryClone for UnixStream {
    fn try_clone_box(&self) -> io::Result<Self> {
        self.try_clone()
    }
}

/// Serve `listener` until the process ends, relaying each connection to
/// `target`. Runs in a background thread.
pub fn unix_to_tcp(path: &Path, target: String) -> io::Result<()> {
    let _ = std::fs::remove_file(path);
    let l = UnixListener::bind(path)?;
    thread::spawn(move || {
        for c in l.incoming().flatten() {
            if let Ok(up) = TcpStream::connect(&target) {
                pump(
                    c,
                    up,
                    |s: &UnixStream| drop(s.shutdown(Shutdown::Write)),
                    |s: &TcpStream| drop(s.shutdown(Shutdown::Write)),
                );
            }
        }
    });
    Ok(())
}

/// Serve on `listen` (host:port) and relay to `target`, in the foreground.
pub fn tcp_to_tcp(listen: &str, target: &str) -> io::Result<()> {
    let l = TcpListener::bind(listen)?;
    for c in l.incoming().flatten() {
        if let Ok(up) = TcpStream::connect(target) {
            pump(
                c,
                up,
                |s: &TcpStream| drop(s.shutdown(Shutdown::Write)),
                |s: &TcpStream| drop(s.shutdown(Shutdown::Write)),
            );
        }
    }
    Ok(())
}

/// `host:port` of a proxy URL such as `http://user:pw@127.0.0.1:3128/`.
pub fn proxy_authority(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let rest = rest.split('/').next()?;
    let rest = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    // A port-less proxy URL means the scheme's port.
    let has_port = match rest.strip_prefix('[') {
        Some(v6) => v6.split_once(']').is_some_and(|(_, p)| p.starts_with(':')),
        None => rest.contains(':'),
    };
    if has_port {
        Some(rest.to_owned())
    } else {
        Some(format!(
            "{rest}:{}",
            if scheme.eq_ignore_ascii_case("https") {
                443
            } else {
                80
            }
        ))
    }
}

/// `url` with its host and port replaced by `authority`; the scheme, any
/// credentials and the path are kept.
pub fn with_authority(url: &str, authority: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let (hostpart, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let userinfo = hostpart
        .rsplit_once('@')
        .map(|(u, _)| format!("{u}@"))
        .unwrap_or_default();
    format!("{scheme}://{userinfo}{authority}{path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn a_relayed_proxy_keeps_its_credentials() {
        assert_eq!(
            with_authority("http://u:p@127.0.0.1:3128", "10.25.127.1:40000"),
            "http://u:p@10.25.127.1:40000"
        );
        assert_eq!(
            with_authority("http://127.0.0.1:3128/", "10.0.0.1:1"),
            "http://10.0.0.1:1/"
        );
        assert_eq!(
            proxy_authority("http://proxy.example").as_deref(),
            Some("proxy.example:80")
        );
        assert_eq!(
            proxy_authority("https://proxy.example").as_deref(),
            Some("proxy.example:443")
        );
        assert_eq!(
            proxy_authority("http://[::1]:3128").as_deref(),
            Some("[::1]:3128")
        );
        assert_eq!(proxy_authority("http://[::1]").as_deref(), Some("[::1]:80"));
    }

    #[test]
    fn authority_from_proxy_urls() {
        assert_eq!(
            proxy_authority("http://127.0.0.1:45823").as_deref(),
            Some("127.0.0.1:45823")
        );
        assert_eq!(
            proxy_authority("http://u:p@10.0.0.1:3128/").as_deref(),
            Some("10.0.0.1:3128")
        );
        assert_eq!(
            proxy_authority("proxy.example:8080").as_deref(),
            Some("proxy.example:8080")
        );
        assert_eq!(
            proxy_authority("http://noport").as_deref(),
            Some("noport:80")
        );
        assert_eq!(proxy_authority("http://"), None);
    }

    #[test]
    fn unix_socket_reaches_a_tcp_server() {
        let srv = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = srv.local_addr().unwrap().to_string();
        thread::spawn(move || {
            let (mut c, _) = srv.accept().unwrap();
            let mut buf = [0u8; 4];
            c.read_exact(&mut buf).unwrap();
            c.write_all(&buf.map(|b| b.to_ascii_uppercase())).unwrap();
        });
        let sock = std::env::temp_dir().join(format!("eb-relay-{}.sock", std::process::id()));
        unix_to_tcp(&sock, target).unwrap();
        let mut u = UnixStream::connect(&sock).unwrap();
        u.write_all(b"ping").unwrap();
        let mut back = [0u8; 4];
        u.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"PING");
        let _ = std::fs::remove_file(&sock);
    }
}
