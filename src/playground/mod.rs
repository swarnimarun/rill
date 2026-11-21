//! Local, offline compiler explorer. Static routes are drawn from in-memory catalogues.
mod http;
mod lessons;
pub(crate) mod worker;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{self, Write},
    net::{Ipv4Addr, TcpListener},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

pub(super) const USAGE: &str = "rill playground [--port <0..65535>] | --check-lessons";

pub(super) fn run(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = args.collect::<Vec<_>>();
    let port = match args.as_slice() {
        [] => 7878,
        [flag] if flag == "--check-lessons" => return lessons::check(),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}\nOpen the printed loopback URL. Port 0 selects an available port. Ctrl-C stops the server.");
            return Ok(());
        }
        [flag, n] if flag == "--port" => n
            .to_str()
            .and_then(|n| n.parse::<u16>().ok())
            .ok_or(USAGE)?,
        _ => return Err(USAGE.into()),
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| format!("E-PLAYGROUND: could not listen on 127.0.0.1:{port}: {e}. Use --port 0 to select an available port."))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let site = Arc::new(Site {
        port,
        lessons: lessons::catalogue()?.to_string(),
        book: crate::docgen::language_book()?,
    });
    let (send, receive) = mpsc::sync_channel(12);
    let receive = Arc::new(Mutex::new(receive));
    for _ in 0..6 {
        let receive = Arc::clone(&receive);
        let site = Arc::clone(&site);
        thread::spawn(move || loop {
            let connection = receive.lock().unwrap().recv();
            let Ok(mut stream) = connection else { break };
            let _ = http::serve(&site, &mut stream);
        });
    }
    println!("Rill playground  http://127.0.0.1:{port}/");
    io::stdout().flush().map_err(|e| e.to_string())?;
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| e.to_string())?;
        let timeout = Some(Duration::from_secs(5));
        stream
            .set_read_timeout(timeout)
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(timeout)
            .map_err(|e| e.to_string())?;
        if let Err(mpsc::TrySendError::Full(mut stream)) = send.try_send(stream) {
            let _ = http::busy(&mut stream);
        }
    }
    Ok(())
}

struct Site {
    port: u16,
    lessons: String,
    book: BTreeMap<String, Vec<u8>>,
}
