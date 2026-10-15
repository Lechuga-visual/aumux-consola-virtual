# tiny_http 0.12.0

Source: https://crates.io/crates/tiny_http/0.12.0
Upstream: https://github.com/tiny-http/tiny-http
Published crate checksum: `389915df6413a2e74fb181895f933386023c71110878cd0825588928e64cdc82`
License: MIT OR Apache-2.0; original licenses retained.

Local patch: `src/util/task_pool.rs`, `TaskPool::spawn`: reserve a waiting worker
for each queued task before deciding whether a new connection needs a new
worker. The previous test of `waiting_tasks == 0` could enqueue multiple
connections for the same still-waking workers. Long-lived keep-alive connections
then occupied every worker while later asset requests stayed queued.

Regression: `desktop/tests/local-http-smoke.py` keeps twelve HTTP connections
alive concurrently across twelve bursts; all assets must arrive within the
socket deadline. Browser suites exercise the console startup as well.
No change to HTTP parsing, licensing or the audio callback.
