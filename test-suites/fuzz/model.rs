use core::fmt;

pub const MAX_OPERATIONS: usize = 8;
pub const OPERATION_WIDTH: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operation {
    pub code: u8,
    pub actor: u8,
    pub asset: u8,
    pub flags: u8,
    pub amount: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeOutcome {
    Empty,
    EmptyProgram,
    TooManyOperations,
    Truncated,
    TrailingBytes,
}

impl fmt::Display for DecodeOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "empty input",
            Self::EmptyProgram => "empty program",
            Self::TooManyOperations => "operation bound exceeded",
            Self::Truncated => "truncated operation",
            Self::TrailingBytes => "trailing bytes",
        })
    }
}

/// Decode a fixed-width operation program. The low nibble makes printable seed
/// files possible while preserving explicit empty, oversize, truncated, and
/// trailing-input outcomes. No input can execute more than MAX_OPERATIONS.
pub fn decode(input: &[u8]) -> Result<Vec<Operation>, DecodeOutcome> {
    let Some(header) = input.first().copied() else {
        return Err(DecodeOutcome::Empty);
    };
    let count = usize::from(header & 0x0f);
    if count == 0 {
        return Err(DecodeOutcome::EmptyProgram);
    }
    if count > MAX_OPERATIONS {
        return Err(DecodeOutcome::TooManyOperations);
    }
    let expected = 1 + count * OPERATION_WIDTH;
    if input.len() < expected {
        return Err(DecodeOutcome::Truncated);
    }
    if input.len() > expected {
        return Err(DecodeOutcome::TrailingBytes);
    }

    let mut operations = Vec::with_capacity(count);
    for bytes in input[1..].chunks_exact(OPERATION_WIDTH) {
        operations.push(Operation {
            code: bytes[0],
            actor: bytes[1],
            asset: bytes[2],
            flags: bytes[3],
            amount: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        });
    }
    Ok(operations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_distinguishes_every_bounded_outcome() {
        assert_eq!(decode(&[]), Err(DecodeOutcome::Empty));
        assert_eq!(decode(&[0]), Err(DecodeOutcome::EmptyProgram));
        assert_eq!(decode(&[9]), Err(DecodeOutcome::TooManyOperations));
        assert_eq!(decode(&[1]), Err(DecodeOutcome::Truncated));
        assert_eq!(
            decode(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(DecodeOutcome::TrailingBytes)
        );
    }

    #[test]
    fn decoder_accepts_exactly_the_maximum_program() {
        let mut input = vec![MAX_OPERATIONS as u8];
        input.resize(1 + MAX_OPERATIONS * OPERATION_WIDTH, 0);
        let operations = decode(&input).expect("maximum-size input must decode");
        assert_eq!(operations.len(), MAX_OPERATIONS);
    }
}
