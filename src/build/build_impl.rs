use simple_fs::SPath;

use crate::Result;

#[derive(Debug, Default)]
pub struct BuildResponse;

pub async fn build(src_path: SPath) -> Result<BuildResponse> {
	let _ = src_path;

	Ok(BuildResponse)
}
