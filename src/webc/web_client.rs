pub struct WebClient {
	reqwest: reqwest::Client,
}

pub fn new_client(opts: impl Into<super::WebClientOptions>) -> crate::Result<WebClient> {
	let _opts = opts.into();
	let reqwest = reqwest::Client::builder().build()?;

	Ok(WebClient { reqwest })
}

impl WebClient {
	pub fn get(&self, request: impl Into<super::WebRequest>) -> crate::Result<()> {
		let request = request.into();
		let _request = self.reqwest.get(request.url);

		Ok(())
	}
}

// region:    --- WebClientOptions

use crate::macros::FromOptional;
use macro_rules_attribute as mra;

#[mra::derive(Debug, Default, FromOptional!)]
pub struct WebClientOptions {}

// endregion: --- WebClientOptions
