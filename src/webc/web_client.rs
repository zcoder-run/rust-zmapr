use reqwest::Url;

#[derive(Clone)]
pub struct WebClient {
	reqwest: reqwest::Client,
}

pub fn new_client(opts: impl Into<super::WebClientOptions>) -> crate::Result<WebClient> {
	let _opts = opts.into();
	let reqwest = reqwest::Client::builder().build()?;

	Ok(WebClient { reqwest })
}

impl WebClient {
	pub async fn get(&self, request: impl Into<super::WebRequest>) -> crate::Result<reqwest::Response> {
		let request = request.into();
		let mut url = Url::parse(&request.url).map_err(crate::Error::custom_from_err)?;
		if let Some(ref params) = request.params {
			let mut query_pairs = url.query_pairs_mut();
			for (name, value) in params {
				query_pairs.append_pair(name.as_ref(), value.as_ref());
			}
			drop(query_pairs);
		}
		let mut req = self.reqwest.get(url);
		if let Some(headers) = request.headers {
			req = req.headers(headers);
		}

		let response = req.send().await?;
		Ok(response)
	}
}

// region:    --- WebClientOptions

use crate::macros::FromOptional;
use macro_rules_attribute as mra;

#[mra::derive(Debug, Default, FromOptional!)]
pub struct WebClientOptions {}

// endregion: --- WebClientOptions
