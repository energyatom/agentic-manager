# axum 0.8.9

Pinned `=0.8.9`, feature `ws`. Used only in `am-server` (AD-13, AD-14).

## What we use

- `Router::new().route(..).layer(middleware::from_fn_with_state(state, guard))
  .with_state(state)`; `axum::serve(listener, router).with_graceful_shutdown(..)`.
- The guard (Host, Origin, cookie + `X-AM-CSRF`) is one `from_fn_with_state`
  middleware over every route, so no route can skip it.
- WebSocket: `WebSocketUpgrade::requested_protocols()` to read the offered
  subprotocols, `.protocols([selected])` to echo the CSRF subprotocol back,
  `.on_upgrade(handler)`; messages are `Message::Text(Utf8Bytes)` (use
  `.into()` from `String`).
- Bind with `tokio::net::TcpListener::bind(127.0.0.1:port)` only.

## Do not

- Do not add tower-http's CORS layer: the API sends no CORS headers (AD-13).
