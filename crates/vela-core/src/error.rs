// crates/vela-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VelaError {
    #[error("invalid WASM: {0}")]
    InvalidWasm(String),

    #[error("not a Component Model WASM: {0}")]
    NotComponent(String),

    #[error("WASM parse error: {0}")]
    Wasm(#[from] wasmparser::BinaryReaderError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        let e = VelaError::InvalidWasm("bad magic".into());
        assert_eq!(e.to_string(), "invalid WASM: bad magic");

        let e = VelaError::NotComponent("expected component".into());
        assert_eq!(
            e.to_string(),
            "not a Component Model WASM: expected component"
        );

        let e = VelaError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert!(e.to_string().contains("missing"));
    }
}
