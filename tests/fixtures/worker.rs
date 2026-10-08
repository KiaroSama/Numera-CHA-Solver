//! Native synthetic worker for ownership regressions, never model-accuracy evidence.
use std::{
    env, fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    time::Duration,
};

fn event(path: &std::path::Path, message: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{message}")?;
    file.flush()
}
fn frame(output: &mut impl Write, json: &[u8]) -> io::Result<()> {
    output.write_all(&(json.len() as u32).to_le_bytes())?;
    output.write_all(json)?;
    output.flush()
}
fn release(control: &mut TcpStream) -> io::Result<()> {
    let mut signal = [0];
    control.read_exact(&mut signal)
}
fn run() -> io::Result<()> {
    let root = PathBuf::from(
        env::var_os("NUMERA_ROOT").ok_or_else(|| io::Error::other("Fixture root is missing."))?,
    );
    let diagnostic = PathBuf::from(
        env::var_os("NUMERA_FIXTURE_DIAGNOSTIC")
            .ok_or_else(|| io::Error::other("Fixture diagnostic path is missing."))?,
    );
    if !diagnostic.starts_with(&root) {
        return Err(io::Error::other(
            "Fixture diagnostic path escapes its root.",
        ));
    }
    event(&diagnostic, "fixture-started")?;
    let address: SocketAddr = env::var("NUMERA_FIXTURE_ADDRESS")
        .map_err(|_| io::Error::other("Fixture address is missing."))?
        .parse()
        .map_err(|_| io::Error::other("Fixture address is invalid."))?;
    if !address.ip().is_loopback() {
        return Err(io::Error::other("Fixture control must be loopback."));
    }
    let mode = env::var("NUMERA_FIXTURE_MODE")
        .map_err(|_| io::Error::other("Fixture mode is missing."))?;
    if !matches!(mode.as_str(), "ready" | "handshake" | "inference" | "crash") {
        return Err(io::Error::other("Unknown fixture mode."));
    }
    let mut control = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    control.set_read_timeout(Some(Duration::from_secs(20)))?;
    control.set_write_timeout(Some(Duration::from_secs(2)))?;
    control.write_all(&std::process::id().to_le_bytes())?;
    control.flush()?;
    event(&diagnostic, "control-connected")?;
    if mode == "handshake" {
        release(&mut control)?;
    }
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    frame(&mut output, br#"{"ready":true}"#)?;
    event(&diagnostic, "ready-sent")?;
    loop {
        let mut length = [0; 4];
        match input.read_exact(&mut length) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let count = u32::from_le_bytes(length) as usize;
        if count > 8 * 1024 * 1024 {
            return Err(io::Error::other(
                "Fixture request exceeds the byte ceiling.",
            ));
        }
        let mut content = vec![0; count];
        input.read_exact(&mut content)?;
        event(&diagnostic, "request-read")?;
        control.write_all(b"I")?;
        control.flush()?;
        if mode == "inference" {
            release(&mut control)?;
        }
        if mode == "crash" {
            return Ok(());
        }
        frame(
            &mut output,
            br#"{"text":"owned","confidence":1.0,"device":"cpu","evidence":{}}"#,
        )?;
    }
}
fn main() {
    if let Err(error) = run() {
        // Only fixed fixture diagnostics are emitted; request bytes and environment values are never logged.
        eprintln!("Owned native fixture failed: {}", error.kind());
        std::process::exit(2);
    }
}
