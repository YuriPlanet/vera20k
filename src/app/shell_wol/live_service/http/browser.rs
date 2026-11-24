//! Browser I/O only. The parent serial driver owns protocol decisions and state.

use super::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = globalThis, js_name = veraLobbyOpen)]
    fn open(origin: &str, service: Option<&str>) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLobbyRequest)]
    fn fetch(
        id: u32,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
        limit: u32,
        headers: Option<&str>,
    ) -> js_sys::Promise;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLobbyCancel)]
    fn cancel(id: u32);
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLobbyDelay)]
    fn timeout(ms: u32) -> js_sys::Promise;
}

/// Dropping an owner cancels only its requests, including those of a retiring visit.
pub(in crate::app::shell_wol::live_service) struct RequestOwner(u32);

impl RequestOwner {
    fn new(service_url: Option<&str>) -> anyhow::Result<Self> {
        // Built-in online play shares the gateway's origin; an external launch
        // names its match API origin (docs/spawn.md).
        open(crate::net::udp::browser_proxy_url(), service_url)
            .map(Self)
            .map_err(|_| anyhow::anyhow!("could not initialize the browser lobby transport"))
    }
}

impl Drop for RequestOwner {
    fn drop(&mut self) {
        cancel(self.0);
    }
}

pub(super) async fn delay(ms: u32) {
    let _ = JsFuture::from(timeout(ms)).await;
}

/// Fetch errors can include URLs or server text. Keep only the existing
/// numeric failure consumed by the caller.
async fn reply(promise: js_sys::Promise) -> Result<(u16, String), RequestFailure> {
    let value = JsFuture::from(promise)
        .await
        .map_err(|_| RequestFailure::Status(0))?;
    let encoded = value.as_string().ok_or(RequestFailure::Status(0))?;
    serde_json::from_str(&encoded).map_err(|_| RequestFailure::Status(0))
}

/// The chooser can ask for public stats before anyone has a session.
pub(in crate::app::shell_wol) async fn browser_stats()
-> anyhow::Result<crate::app::shell_wol::login_info::LobbyStats> {
    use crate::app::shell_wol::login_info::{MAX_STATS_BYTES, STATS_PATH};
    let owner = RequestOwner::new(None)?;
    let (status, body) = reply(fetch(
        owner.0,
        STATS_PATH,
        None,
        None,
        MAX_STATS_BYTES,
        None,
    ))
    .await
    .map_err(|_| anyhow::anyhow!("cannot read /v1/stats"))?;
    anyhow::ensure!(status == 200, "/v1/stats answered {status}");
    Ok(serde_json::from_str(&body)?)
}

impl Client {
    pub(super) async fn request(
        &self,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, RequestFailure> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        let body = body.map(Value::to_string);
        let headers =
            (path == "/v1/sessions").then(|| json!(sign_in_headers(&self.changes)).to_string());
        let (status, body) = reply(fetch(
            self.browser,
            path,
            self.token.as_deref(),
            body.as_deref(),
            MAX_BODY_BYTES as u32,
            headers.as_deref(),
        ))
        .await?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        if !(200..300).contains(&status) {
            return Err(RequestFailure::answer(path, status, body.as_bytes()));
        }
        serde_json::from_str(&body).map_err(|_| RequestFailure::Status(0))
    }
}

pub(in crate::app::shell_wol::live_service) fn start(
    input: Receiver<Request>,
    output: Sender<Response>,
    stopping: Arc<AtomicBool>,
    service_url: Option<&str>,
) -> anyhow::Result<RequestOwner> {
    let owner = RequestOwner::new(service_url)?;
    let id = owner.0;
    let client = Client::new(Arc::clone(&stopping), id);
    let (jobs, translations) = std::sync::mpsc::channel::<TranslateJob>();
    let mut translator = client.sibling();
    let translated = output.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while !translator.stopping.load(Ordering::Acquire) {
            let job = match next_request(&translations).await {
                Ok(Some(job)) => job,
                Ok(None) => continue,
                Err(()) => break,
            };
            let response = translator.translate(job).await;
            if translator.stopping.load(Ordering::Acquire) || translated.send(response).is_err() {
                break;
            }
        }
    });
    // The existing retirement owner sets stopping before waiting for all
    // response senders to close. Cancel an in-flight fetch without waiting for
    // its twelve-second network timeout; this task owns no response sender.
    let watch = Arc::clone(&stopping);
    wasm_bindgen_futures::spawn_local(async move {
        while !watch.load(Ordering::Acquire) {
            delay(50).await;
        }
        cancel(id);
    });
    wasm_bindgen_futures::spawn_local(async move {
        run_loop(client, input, output, Some(jobs)).await;
        stopping.store(true, Ordering::Release);
        cancel(id);
    });
    Ok(owner)
}
