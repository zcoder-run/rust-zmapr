macro_rules! FromOptional {
	(
		$(#[$attr:meta])*
		$vis:vis struct $name:ident {
			$($fields:tt)*
		}
	) => {
		impl From<Option<$name>> for $name {
			fn from(value: Option<$name>) -> Self {
				value.unwrap_or_default()
			}
		}
	};
}

pub(crate) use FromOptional;
