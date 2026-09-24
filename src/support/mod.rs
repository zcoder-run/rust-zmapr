pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
	let hash = blake3::hash(bytes);
	bs58::encode(hash.as_bytes()).into_string()
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_support_hash_bytes_stable() -> Result<()> {
		// -- Setup & Fixtures
		let bytes = b"shared content hash";

		// -- Exec
		let hash = hash_bytes(bytes);

		// -- Check
		assert_eq!(hash, bs58::encode(blake3::hash(bytes).as_bytes()).into_string());

		Ok(())
	}
}

// endregion: --- Tests
