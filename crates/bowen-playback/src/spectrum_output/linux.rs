use super::pcm::PcmStream;
use crate::spectrum::SpectrumLevels;
use std::{
    io::{self, Read},
    os::fd::AsRawFd,
    process::{Child, ChildStdout, Command, Stdio},
};

pub(super) struct Capture {
    child: Child,
    stdout: Option<ChildStdout>,
    pcm: PcmStream,
}

impl Capture {
    pub fn open() -> io::Result<Self> {
        let child = Command::new("parec")
            .args([
                "--raw",
                "--device=@DEFAULT_MONITOR@",
                "--client-name=Bowen Music",
                "--stream-name=Lyrics spectrum",
                "--format=float32le",
                "--rate=48000",
                "--channels=2",
                "--latency-msec=35",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut capture = Self {
            child,
            stdout: None,
            pcm: PcmStream::new(),
        };
        let stdout = capture
            .child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("parec stdout is unavailable"))?;
        let fd = stdout.as_raw_fd();
        // The pipe is owned by stdout and remains open throughout these calls.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        capture.stdout = Some(stdout);
        Ok(capture)
    }

    pub fn poll(&mut self) -> io::Result<Option<SpectrumLevels>> {
        let stdout = self
            .stdout
            .as_mut()
            .expect("capture must have a pipe after opening");
        let mut buffer = [0; 8192];
        for _ in 0..8 {
            match stdout.read(&mut buffer) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(length) => self.pcm.push(&buffer[..length]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(self.pcm.take_levels())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
