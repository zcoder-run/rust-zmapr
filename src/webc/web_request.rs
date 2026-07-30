use crate::Result;

// region:    --- Types

pub type WebHeaders = reqwest::header::HeaderMap;
pub type WebParams = Vec<(String, String)>;

pub struct WebRequest {
	pub url: String,
	pub headers: Option<WebHeaders>,
	pub params: Option<WebParams>,
}

// endregion: --- Types

impl WebRequest {
	pub fn new(url: impl Into<String>) -> Self {
		Self {
			url: url.into(),
			headers: None,
			params: None,
		}
	}
}

impl WebRequest {
	pub fn with_headers(mut self, headers: WebHeaders) -> Self {
		self.headers = Some(headers);
		self
	}

	pub fn append_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
		let name = reqwest::header::HeaderName::from_bytes(name.into().as_bytes())?;
		let value = value.into().parse::<reqwest::header::HeaderValue>()?;
		self.headers.get_or_insert_default().append(name, value);
		Ok(self)
	}

	pub fn with_params(mut self, params: impl IntoIterator<Item = (String, String)>) -> Self {
		self.params = Some(params.into_iter().collect());
		self
	}

	pub fn append_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
		self.params.get_or_insert_default().push((name.into(), value.into()));
		self
	}

	pub fn append_params(mut self, params: impl IntoIterator<Item = (String, String)>) -> Self {
		self.params.get_or_insert_default().extend(params);
		self
	}
}

// region:    --- Froms

impl From<String> for WebRequest {
	fn from(url: String) -> Self {
		Self::new(url)
	}
}

impl From<&str> for WebRequest {
	fn from(url: &str) -> Self {
		url.to_owned().into()
	}
}

impl From<&String> for WebRequest {
	fn from(url: &String) -> Self {
		url.to_owned().into()
	}
}

// endregion: --- Froms
