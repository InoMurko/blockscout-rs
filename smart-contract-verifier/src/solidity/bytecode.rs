use super::{
    errors::{BytecodeInitError, VerificationErrorKind},
    metadata::MetadataHash,
};
use crate::{types::Mismatch, DisplayBytes};
use bytes::{Buf, Bytes};
use ethers_solc::{artifacts::Contract, Artifact};
use std::str::FromStr;

/// Combine creation_tx_input and deployed_bytecode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bytecode {
    /// Raw bytecode bytes used in contract creation transaction
    creation_tx_input: Bytes,
    /// Raw deployed bytecode bytes
    deployed_bytecode: Bytes,
}

impl Bytecode {
    pub fn new(
        creation_tx_input: &str,
        deployed_bytecode: &str,
    ) -> Result<Self, BytecodeInitError> {
        let creation_tx_input = DisplayBytes::from_str(creation_tx_input)
            .map_err(|_| BytecodeInitError::InvalidCreationTxInput(creation_tx_input.to_string()))?
            .0;

        let deployed_bytecode = DisplayBytes::from_str(deployed_bytecode)
            .map_err(|_| BytecodeInitError::InvalidDeployedBytecode(deployed_bytecode.to_string()))?
            .0;

        Self::from_bytes(creation_tx_input, deployed_bytecode)
    }

    pub fn from_bytes(
        creation_tx_input: Bytes,
        deployed_bytecode: Bytes,
    ) -> Result<Self, BytecodeInitError> {
        if creation_tx_input.is_empty() {
            return Err(BytecodeInitError::EmptyCreationTxInput);
        }

        if deployed_bytecode.is_empty() {
            return Err(BytecodeInitError::EmptyDeployedBytecode);
        }

        Ok(Self {
            creation_tx_input,
            deployed_bytecode,
        })
    }

    pub fn creation_tx_input(&self) -> &Bytes {
        &self.creation_tx_input
    }
}

impl TryFrom<&Contract> for Bytecode {
    type Error = BytecodeInitError;

    fn try_from(contract: &Contract) -> Result<Self, Self::Error> {
        let deployed_bytecode = {
            contract.get_deployed_bytecode_bytes().ok_or_else(|| {
                let bytecode = contract
                    .get_deployed_bytecode_object()
                    .unwrap_or_default()
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                BytecodeInitError::InvalidDeployedBytecode(bytecode)
            })?
        };
        let creation_tx_input = {
            contract.get_bytecode_bytes().ok_or_else(|| {
                let bytecode = contract
                    .get_bytecode_object()
                    .unwrap_or_default()
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                BytecodeInitError::InvalidCreationTxInput(bytecode)
            })?
        };
        Bytecode::from_bytes(creation_tx_input.0.clone(), deployed_bytecode.0.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodePart {
    Main {
        raw: Bytes,
    },
    Metadata {
        metadata_raw: Bytes,
        metadata: MetadataHash,
        metadata_length_raw: Bytes,
    },
}

impl BytecodePart {
    pub fn size(&self) -> usize {
        match self {
            BytecodePart::Main { raw } => raw.len(),
            BytecodePart::Metadata {
                metadata_raw,
                metadata_length_raw,
                ..
            } => metadata_raw.len() + metadata_length_raw.len(),
        }
    }
}

/// Encapsulates result of local source code compilation.
/// Splits compiled creation transaction input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBytecode {
    bytecode: Bytecode,
    creation_tx_input_parts: Vec<BytecodePart>,
}

impl LocalBytecode {
    /// Initializes a new [`LocalBytecode`] struct.
    ///
    /// `bytecode` and `bytecode_modified` must differ only in metadata hashes.
    ///
    /// Any error here is [`VerificationErrorKind::InternalError`], as both original
    /// and modified bytecodes are obtained as a result of local compilation.
    pub fn new(
        bytecode: Bytecode,
        bytecode_modified: Bytecode,
    ) -> Result<Self, VerificationErrorKind> {
        let creation_tx_input_parts = Self::split(
            &bytecode.creation_tx_input,
            &bytecode_modified.creation_tx_input,
        )?;

        Ok(Self {
            bytecode,
            creation_tx_input_parts,
        })
    }

    pub fn creation_tx_input(&self) -> &Bytes {
        &self.bytecode.creation_tx_input
    }

    pub fn creation_tx_input_parts(&self) -> &Vec<BytecodePart> {
        &self.creation_tx_input_parts
    }

    /// Splits bytecode onto [`BytecodePart`]s using bytecode with modified metadata hashes.
    ///
    /// Any error here is [`VerificationErrorKind::InternalError`], as both original
    /// and modified bytecodes are obtained as a result of local compilation.
    fn split(
        raw: &Bytes,
        raw_modified: &Bytes,
    ) -> Result<Vec<BytecodePart>, VerificationErrorKind> {
        if raw.len() != raw_modified.len() {
            return Err(VerificationErrorKind::InternalError(format!(
                "bytecode and modified bytecode length mismatch: {}",
                Mismatch::new(raw.len(), raw_modified.len())
            )));
        }

        let parts_total_size = |parts: &Vec<BytecodePart>| -> usize {
            parts.iter().fold(0, |size, el| size + el.size())
        };

        let mut result = Vec::new();

        let mut i = 0usize;
        while i < raw.len() {
            let decoded = Self::parse_bytecode_parts(&raw.slice(i..raw.len()), &raw_modified[i..])?;
            let decoded_size = parts_total_size(&decoded);
            result.extend(decoded);

            i += decoded_size;
        }

        Ok(result)
    }

    /// Finds the next [`BytecodePart`]s into a series of bytes.
    ///
    /// Parses at most one [`BytecodePart::Main`] and one [`BytecodePart::Metadata`].
    fn parse_bytecode_parts(
        raw: &Bytes,
        raw_modified: &[u8],
    ) -> Result<Vec<BytecodePart>, VerificationErrorKind> {
        let mut parts = Vec::new();

        let len = raw.len();

        // search for the first non-matching byte
        let mut index = raw
            .iter()
            .zip(raw_modified.iter())
            .position(|(a, b)| a != b);

        // There is some non-matching byte - part of the metadata part byte.
        if let Some(mut i) = index {
            // `i` is the first different byte. The metadata hash itself started somewhere earlier
            // (at least for "a1"/"a2" indicating number of elements in cbor mapping).
            // Next steps are trying to find that beginning.

            let mut result = MetadataHash::from_cbor(&raw[i..]);
            while result.is_err() {
                // It is the beginning of the bytecode segment but no metadata hash has been parsed
                if i == 0 {
                    return Err(VerificationErrorKind::InternalError(
                        "failed to parse bytecode part".into(),
                    ));
                }
                i -= 1;

                result = MetadataHash::from_cbor(&raw[i..]);
            }

            let (metadata, metadata_length) = result.unwrap();

            if len < i + metadata_length + 2 {
                return Err(VerificationErrorKind::InternalError(
                    "failed to parse metadata length".into(),
                ));
            }
            // Decode length of metadata hash representation
            let metadata_length_raw = raw.slice((i + metadata_length)..(i + metadata_length + 2));
            let encoded_metadata_length = metadata_length_raw.clone().get_u16() as usize;
            if encoded_metadata_length != metadata_length {
                return Err(VerificationErrorKind::InternalError(format!(
                    "encoded metadata length does not correspond to actual metadata length: {}",
                    Mismatch::new(metadata_length, encoded_metadata_length)
                )));
            }

            parts.push(BytecodePart::Metadata {
                metadata_raw: raw.slice(i..(i + metadata_length)),
                metadata,
                metadata_length_raw,
            });

            // Update index to point where metadata part begins
            index = Some(i);
        }

        // If there is something before metadata part (if any)
        // belongs to main part
        let i = index.unwrap_or(len);
        if i > 0 {
            parts.insert(
                0,
                BytecodePart::Main {
                    raw: raw.slice(0..i),
                },
            )
        }

        Ok(parts)
    }
}

#[cfg(test)]
mod local_bytecode_initialization_tests {
    use super::*;
    use const_format::concatcp;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    const CREATION_TX_INPUT_MAIN_PART_1: &'static str = "608060405234801561001057600080fd5b506040518060200161002190610050565b6020820181038252601f19601f820116604052506000908051906020019061004a92919061005c565b5061015f565b605c806101ac83390190565b8280546100689061012e565b90600052602060002090601f01602090048101928261008a57600085556100d1565b82601f106100a357805160ff19168380011785556100d1565b828001600101855582156100d1579182015b828111156100d05782518255916020019190600101906100b5565b5b5090506100de91906100e2565b5090565b5b808211156100fb5760008160009055506001016100e3565b5090565b7f4e487b7100000000000000000000000000000000000000000000000000000000600052602260045260246000fd5b6000600282049050600182168061014657607f821691505b602082108103610159576101586100ff565b5b50919050565b603f8061016d6000396000f3fe6080604052600080fdfe";
    const CREATION_TX_INPUT_MAIN_PART_2: &'static str =
        "6080604052348015600f57600080fd5b50603f80601d6000396000f3fe6080604052600080fdfe";

    const DEPLOYED_BYTECODE_MAIN_PART_1: &'static str = "6080604052600080fdfe";
    const DEPLOYED_BYTECODE_MAIN_PART_2: &'static str = "6080604052600080cafe";

    const METADATA_PART_1: &'static str = "a26469706673582212202e82fb6222f966f0e56dc49cd1fb8a6b5eac9bdf74f62b8a5e9d8812901095d664736f6c634300080e0033";
    const METADATA_PART_2: &'static str = "a2646970667358221220bd9f7fd5fb164e10dd86ccc9880d27a177e74ba873e6a9b97b6c4d7062b26ff064736f6c634300080e0033";

    const METADATA_PART1_MODIFIED: &'static str = "a264697066735822122028c67e368422bc9c0b12226a099aa62a1facd39b08a84427d7f3efe1e37029b864736f6c634300080e0033";
    const METADATA_PART2_MODIFIED: &'static str = "a26469706673582212206b331720b143820ca2e65d7db53a1b005672433fcb7f2da3ab539851bddc226a64736f6c634300080e0033";

    const DEFAULT_DEPLOYED_BYTECODE: &'static str =
        concatcp!(DEPLOYED_BYTECODE_MAIN_PART_1, METADATA_PART_1);
    const DEFAULT_DEPLOYED_BYTECODE_MODIFIED: &'static str =
        concatcp!(DEPLOYED_BYTECODE_MAIN_PART_1, METADATA_PART1_MODIFIED);

    fn main_bytecode_part(raw: &str) -> BytecodePart {
        let raw = DisplayBytes::from_str(raw)
            .expect("Main bytecode part is invalid hex")
            .0;
        BytecodePart::Main { raw }
    }

    fn metadata_bytecode_part(raw: &str) -> BytecodePart {
        let raw = DisplayBytes::from_str(raw)
            .expect("Metadata bytecode part is invalid hex")
            .0;
        let (metadata, len) =
            MetadataHash::from_cbor(&raw).expect("Metadata bytecode part is not metadata");
        if raw.len() != len + 2 {
            panic!("Metadata bytecode part has invalid length");
        }
        let metadata_length_raw = raw.slice(len..raw.len());
        BytecodePart::Metadata {
            metadata_raw: raw.slice(0..len),
            metadata,
            metadata_length_raw,
        }
    }

    #[test]
    fn without_metadata() {
        let creation_tx_input = format!("{}", CREATION_TX_INPUT_MAIN_PART_1);
        let deployed_bytecode = format!("{}", DEPLOYED_BYTECODE_MAIN_PART_1);

        let creation_tx_input_modified = format!("{}", CREATION_TX_INPUT_MAIN_PART_1);
        let deployed_bytecode_modified = format!("{}", DEPLOYED_BYTECODE_MAIN_PART_1);

        let bytecode = Bytecode::new(&creation_tx_input, &deployed_bytecode)
            .expect("Bytecode initialization failed");
        let bytecode_modified =
            Bytecode::new(&creation_tx_input_modified, &deployed_bytecode_modified)
                .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        let local_bytecode = local_bytecode.expect("Initialization of local bytecode failed");
        assert_eq!(bytecode, local_bytecode.bytecode, "Invalid bytecode");
        assert_eq!(
            vec![main_bytecode_part(CREATION_TX_INPUT_MAIN_PART_1)],
            local_bytecode.creation_tx_input_parts,
            "Invalid bytecode parts"
        );
    }

    #[test]
    fn with_one_metadata() {
        let creation_tx_input = format!("{}{}", CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART_1);
        let deployed_bytecode = format!("{}{}", DEPLOYED_BYTECODE_MAIN_PART_1, METADATA_PART_1);

        let creation_tx_input_modified = format!(
            "{}{}",
            CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART1_MODIFIED
        );
        let deployed_bytecode_modified = format!(
            "{}{}",
            DEPLOYED_BYTECODE_MAIN_PART_1, METADATA_PART1_MODIFIED
        );

        let bytecode = Bytecode::new(&creation_tx_input, &deployed_bytecode)
            .expect("Bytecode initialization failed");
        let bytecode_modified =
            Bytecode::new(&creation_tx_input_modified, &deployed_bytecode_modified)
                .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        let local_bytecode = local_bytecode.expect("Initialization of local bytecode failed");
        assert_eq!(bytecode, local_bytecode.bytecode, "Invalid bytecode");
        assert_eq!(
            vec![
                main_bytecode_part(CREATION_TX_INPUT_MAIN_PART_1),
                metadata_bytecode_part(METADATA_PART_1)
            ],
            local_bytecode.creation_tx_input_parts,
            "Invalid bytecode parts"
        );
    }

    #[test]
    fn with_two_metadata() {
        let creation_tx_input = format!(
            "{}{}{}{}",
            CREATION_TX_INPUT_MAIN_PART_1,
            METADATA_PART_1,
            CREATION_TX_INPUT_MAIN_PART_2,
            METADATA_PART_2
        );
        let deployed_bytecode = format!(
            "{}{}{}{}",
            DEPLOYED_BYTECODE_MAIN_PART_1,
            METADATA_PART_1,
            DEPLOYED_BYTECODE_MAIN_PART_2,
            METADATA_PART_2
        );

        let creation_tx_input_modified = format!(
            "{}{}{}{}",
            CREATION_TX_INPUT_MAIN_PART_1,
            METADATA_PART1_MODIFIED,
            CREATION_TX_INPUT_MAIN_PART_2,
            METADATA_PART2_MODIFIED
        );
        let deployed_bytecode_modified = format!(
            "{}{}{}{}",
            DEPLOYED_BYTECODE_MAIN_PART_1,
            METADATA_PART1_MODIFIED,
            DEPLOYED_BYTECODE_MAIN_PART_2,
            METADATA_PART2_MODIFIED
        );

        let bytecode = Bytecode::new(&creation_tx_input, &deployed_bytecode)
            .expect("Bytecode initialization failed");
        let bytecode_modified =
            Bytecode::new(&creation_tx_input_modified, &deployed_bytecode_modified)
                .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        let local_bytecode = local_bytecode.expect("Initialization of local bytecode failed");
        assert_eq!(bytecode, local_bytecode.bytecode, "Invalid bytecode");
        assert_eq!(
            vec![
                main_bytecode_part(CREATION_TX_INPUT_MAIN_PART_1),
                metadata_bytecode_part(METADATA_PART_1),
                main_bytecode_part(CREATION_TX_INPUT_MAIN_PART_2),
                metadata_bytecode_part(METADATA_PART_2),
            ],
            local_bytecode.creation_tx_input_parts,
            "Invalid bytecode parts"
        );
    }

    #[test]
    fn with_two_metadata_but_one_main_part() {
        let creation_tx_input = format!(
            "{}{}{}",
            CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART_1, METADATA_PART_2
        );
        let deployed_bytecode = format!(
            "{}{}{}",
            DEPLOYED_BYTECODE_MAIN_PART_1, METADATA_PART_1, METADATA_PART_2
        );

        let creation_tx_input_modified = format!(
            "{}{}{}",
            CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART1_MODIFIED, METADATA_PART2_MODIFIED
        );
        let deployed_bytecode_modified = format!(
            "{}{}{}",
            DEPLOYED_BYTECODE_MAIN_PART_1, METADATA_PART1_MODIFIED, METADATA_PART2_MODIFIED
        );

        let bytecode = Bytecode::new(&creation_tx_input, &deployed_bytecode)
            .expect("Bytecode initialization failed");
        let bytecode_modified =
            Bytecode::new(&creation_tx_input_modified, &deployed_bytecode_modified)
                .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        let local_bytecode = local_bytecode.expect("Initialization of local bytecode failed");
        assert_eq!(bytecode, local_bytecode.bytecode, "Invalid bytecode");
        assert_eq!(
            vec![
                main_bytecode_part(CREATION_TX_INPUT_MAIN_PART_1),
                metadata_bytecode_part(METADATA_PART_1),
                metadata_bytecode_part(METADATA_PART_2),
            ],
            local_bytecode.creation_tx_input_parts,
            "Invalid bytecode parts"
        );
    }

    #[test]
    fn with_different_lengths_should_fail() {
        let creation_tx_input = format!("{}{}", CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART_1);

        // additional byte
        let creation_tx_input_modified = format!(
            "{}{}12",
            CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART1_MODIFIED
        );

        let bytecode = Bytecode::new(&creation_tx_input, DEFAULT_DEPLOYED_BYTECODE)
            .expect("Bytecode initialization failed");
        let bytecode_modified = Bytecode::new(
            &creation_tx_input_modified,
            DEFAULT_DEPLOYED_BYTECODE_MODIFIED,
        )
        .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        assert!(
            local_bytecode.is_err(),
            "Should fail, but: {:?}",
            local_bytecode.unwrap()
        );
        match local_bytecode.unwrap_err() {
            VerificationErrorKind::InternalError(error) => {
                assert!(
                    error.contains("length mismatch"),
                    "Invalid error message: {}",
                    error
                )
            }
            _ => panic!("Invalid error"),
        }
    }

    #[test]
    fn with_invalid_metadata_should_fail() {
        let creation_tx_input = format!("{}cafe{}", CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART_1);

        let creation_tx_input_modified = format!(
            "{}abcd{}",
            CREATION_TX_INPUT_MAIN_PART_1, METADATA_PART1_MODIFIED
        );

        let bytecode = Bytecode::new(&creation_tx_input, DEFAULT_DEPLOYED_BYTECODE)
            .expect("Bytecode initialization failed");
        let bytecode_modified = Bytecode::new(
            &creation_tx_input_modified,
            DEFAULT_DEPLOYED_BYTECODE_MODIFIED,
        )
        .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        assert!(
            local_bytecode.is_err(),
            "Should fail, but: {:?}",
            local_bytecode.unwrap()
        );
        match local_bytecode.unwrap_err() {
            VerificationErrorKind::InternalError(error) => {
                assert!(
                    error.contains("failed to parse bytecode part"),
                    "Invalid error message: {}",
                    error
                )
            }
            _ => panic!("Invalid error"),
        }
    }

    #[test]
    fn with_absent_metadata_length_should_fail() {
        let creation_tx_input = format!(
            "{}{}",
            CREATION_TX_INPUT_MAIN_PART_1,
            &METADATA_PART_1[..METADATA_PART_1.len() - 2]
        );

        let creation_tx_input_modified = format!(
            "{}{}",
            CREATION_TX_INPUT_MAIN_PART_1,
            &METADATA_PART1_MODIFIED[..METADATA_PART1_MODIFIED.len() - 2]
        );

        let bytecode = Bytecode::new(&creation_tx_input, DEFAULT_DEPLOYED_BYTECODE)
            .expect("Bytecode initialization failed");
        let bytecode_modified = Bytecode::new(
            &creation_tx_input_modified,
            DEFAULT_DEPLOYED_BYTECODE_MODIFIED,
        )
        .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        assert!(
            local_bytecode.is_err(),
            "Should fail, but: {:?}",
            local_bytecode.unwrap()
        );
        match local_bytecode.unwrap_err() {
            VerificationErrorKind::InternalError(error) => {
                assert!(error.contains(""), "Invalid error message: {}", error)
            }
            _ => panic!("failed to parse metadata length"),
        }
    }

    #[test]
    fn with_invalid_metadata_length_should_fail() {
        let creation_tx_input = format!(
            "{}{}{}",
            CREATION_TX_INPUT_MAIN_PART_1,
            &METADATA_PART_1[..METADATA_PART_1.len() - 4],
            "0031"
        );

        let creation_tx_input_modified = format!(
            "{}{}{}",
            CREATION_TX_INPUT_MAIN_PART_1,
            &METADATA_PART1_MODIFIED[..METADATA_PART1_MODIFIED.len() - 4],
            "0031"
        );

        let bytecode = Bytecode::new(&creation_tx_input, DEFAULT_DEPLOYED_BYTECODE)
            .expect("Bytecode initialization failed");
        let bytecode_modified = Bytecode::new(
            &creation_tx_input_modified,
            DEFAULT_DEPLOYED_BYTECODE_MODIFIED,
        )
        .expect("Modified bytecode initialization failed");

        let local_bytecode = LocalBytecode::new(bytecode.clone(), bytecode_modified);

        assert!(
            local_bytecode.is_err(),
            "Should fail, but: {:?}",
            local_bytecode.unwrap()
        );
        match local_bytecode.unwrap_err() {
            VerificationErrorKind::InternalError(error) => {
                assert!(
                    error.contains(
                        "encoded metadata length does not correspond to actual metadata length"
                    ),
                    "Invalid error message: {}",
                    error
                )
            }
            _ => panic!("failed to parse metadata length"),
        }
    }
}
