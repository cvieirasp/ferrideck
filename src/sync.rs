//! Offline-first synchronization with the AWS Lambda backend over HTTP.
//! Pushes local changes, applies remote ones and resolves conflicts by
//! last-write-wins on `updated_at`.
