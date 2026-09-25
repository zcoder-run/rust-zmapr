# Error handling

Fallible crate operations use [`Result<T>`], which aliases `core::result::Result<T, Error>`. The [`Error`] enum groups application and workflow failures with errors from external operations.

`Error` implements `std::error::Error`. Its `Display` output uses the enum's debug representation. String values can be converted into `Error::Custom`, and the external error types represented by dedicated variants can be converted into `Error`.

## Application and workflow errors

The application-specific variants describe configuration, processing, and state failures:

- [`Error::InvalidConfiguration`] reports invalid process configuration.

- [`Error::Unsupported`] reports an unsupported operation or input.

- [`Error::MissingTag`] reports a required tag missing from a response.

- [`Error::MalformedResponse`] reports a response that does not match the expected format.

- [`Error::TaskJoin`] reports a background task that did not complete successfully.

- [`Error::InvalidCache`] reports cache content that is invalid or unusable.

- [`Error::MalformedState`] reports state that does not match the expected format.

Use [`Error::Custom`] for application-defined messages. [`Error::custom`] constructs one from a string-convertible value. [`Error::custom_from_err`] converts another error to its display text; the original error type is not retained.

## External errors

Dedicated variants represent failures from I/O, `simple_fs`, HTTP requests, and invalid HTTP header names or values. These variants retain their respective error values, allowing callers to match on the specific failure category.
