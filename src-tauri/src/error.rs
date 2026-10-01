use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    /// The hidraw node is gone or unusable; the connection should be dropped.
    Io(io::Error),
    /// The mic answered with a status other than ACK.
    Refused { request: String, status: u8 },
    Timeout,
    Invalid(String),
    Mixer(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "lost connection to the microphone: {e}"),
            Error::Refused { request, status } => {
                let why = match status {
                    0x45 => "value out of range",
                    0x4E => "not supported",
                    _ => "unknown status",
                };
                write!(f, "microphone refused {request}: {why} ({status:#04x})")
            }
            Error::Timeout => write!(f, "microphone did not answer"),
            Error::Invalid(what) => write!(f, "{what}"),
            Error::Mixer(what) => write!(f, "ALSA mixer: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}
