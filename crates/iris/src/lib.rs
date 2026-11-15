//! Placeholder from the environment setup. The first build item replaces it.

/// Return the version line that `iris --version` prints.
pub fn version_line() -> String {
    format!("iris {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::version_line;

    #[test]
    fn version_line_names_the_program() {
        assert!(version_line().starts_with("iris "));
    }
}
