use craft_engine::Extraction;
use serde_json::Value;
use std::{io, time::Duration};

/// Per-member extraction limits. Inventory and hashing remain outside these limits.
pub struct Budget {
    max_files: u64,
    max_bytes: u64,
    max_output_bytes: u64,
    max_duration_ms: u64,
    pub attempted: u64,
    pub bytes: u64,
    pub output_bytes: u64,
}

impl Budget {
    pub fn from_input(input: &Value) -> Result<Self, String> {
        let limit = |name| match input.get(name) {
            None => Ok(u64::MAX),
            Some(value) => value
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{name} must be a positive integer")),
        };
        Ok(Self {
            max_files: limit("maxFiles")?,
            max_bytes: limit("maxBytes")?,
            max_output_bytes: limit("maxOutputBytes")?,
            max_duration_ms: limit("maxDurationMs")?,
            attempted: 0,
            bytes: 0,
            output_bytes: 0,
        })
    }

    pub fn start(&mut self, bytes: u64, elapsed: Duration) -> Result<(), &'static str> {
        if self.attempted >= self.max_files {
            return Err("maxFiles");
        }
        if elapsed.as_millis() >= self.max_duration_ms as u128 {
            return Err("maxDurationMs");
        }
        if bytes > self.max_bytes - self.bytes {
            return Err("maxBytes");
        }
        if self.output_bytes >= self.max_output_bytes {
            return Err("maxOutputBytes");
        }
        self.attempted += 1;
        self.bytes += bytes;
        Ok(())
    }

    pub fn accept(&mut self, extraction: &Extraction) -> Result<bool, String> {
        // Count compact JSON without allocating a second copy of the extraction.
        let mut counter = ByteCounter(0);
        serde_json::to_writer(&mut counter, extraction).map_err(|e| e.to_string())?;
        if counter.0 > self.max_output_bytes - self.output_bytes {
            return Ok(false);
        }
        self.output_bytes += counter.0;
        Ok(true)
    }
}

struct ByteCounter(u64);
impl io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn elapsed_boundary_stops_new_extractions_without_charging_them() {
        let mut budget = Budget::from_input(&json!({"maxDurationMs": 5})).unwrap();
        assert_eq!(budget.start(10, Duration::from_millis(4)), Ok(()));
        assert_eq!(
            budget.start(10, Duration::from_millis(5)),
            Err("maxDurationMs")
        );
        assert_eq!(budget.attempted, 1);
        assert_eq!(budget.bytes, 10);
    }

    #[test]
    fn output_limit_counts_utf8_and_json_escaping_without_partial_acceptance() {
        let extraction = Extraction {
            path: "é\".js".into(),
            hash: "a".repeat(64),
            provider: "test".into(),
            evidence: vec![],
        };
        let size = serde_json::to_vec(&extraction).unwrap().len() as u64;
        let mut budget = Budget::from_input(&json!({"maxOutputBytes": size - 1})).unwrap();
        assert!(!budget.accept(&extraction).unwrap());
        assert_eq!(budget.output_bytes, 0);
        let mut budget = Budget::from_input(&json!({"maxOutputBytes": size})).unwrap();
        assert!(budget.accept(&extraction).unwrap());
        assert_eq!(budget.output_bytes, size);
        assert_eq!(budget.start(1, Duration::ZERO), Err("maxOutputBytes"));
    }
}
