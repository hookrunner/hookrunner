use aeronet_websocket::{
    server::HandshakeHandler,
    tungstenite::{handshake::server::ErrorResponse, http::StatusCode},
};
use hookrunner_shared::SIMULATION_BUILD;

#[expect(
    clippy::result_large_err,
    reason = "Tungstenite requires an unboxed HTTP error response"
)]
pub fn handshake() -> HandshakeHandler {
    HandshakeHandler::new(|request, response| {
        let builds: Vec<_> =
            url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
                .filter(|(key, _)| key == "build")
                .map(|(_, value)| value)
                .collect();
        if builds.len() == 1 && builds[0] == SIMULATION_BUILD {
            return Ok(response);
        }
        let mut error = ErrorResponse::new(Some("Build mismatch; load the current client.".into()));
        *error.status_mut() = StatusCode::CONFLICT;
        error
            .headers_mut()
            .insert("Cache-Control", "no-store".parse().unwrap());
        Err(error)
    })
}
