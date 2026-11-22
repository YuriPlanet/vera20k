//! Map files and chunks have one serial worker per sign-in. Only the control
//! worker applies event batches and handles authentication and cursor failures.

use super::*;
use crate::map::source::TransferResource;
use std::sync::{Weak, mpsc};

pub(super) struct Job {
    pub(super) transfer: u64,
    pub(super) active: Weak<()>,
    pub(super) after: u64,
    pub(super) io: MapIo,
    pub(super) completed: Sender<Request>,
}

pub(super) struct Worker {
    token: String,
    lifetime: Arc<()>,
    jobs: Sender<Job>,
}

impl Worker {
    pub(super) fn start(control: &Client) -> Option<Self> {
        let token = control.token.as_ref()?;
        let mut client = control.sibling();
        client.token = Some(token.clone());
        let (jobs, input) = mpsc::channel();
        let lifetime = Arc::new(());
        let visit = Arc::downgrade(&lifetime);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        std::thread::Builder::new()
            .name("vera-lobby-map".into())
            .spawn(move || {
                for job in input {
                    client.map_job(job, &visit);
                }
            })
            .ok()?;
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        wasm_bindgen_futures::spawn_local(async move {
            while visit.strong_count() != 0 && !client.stopping.load(Ordering::Acquire) {
                match next_request(&input).await {
                    Ok(Some(job)) => client.map_job(job, &visit).await,
                    Ok(None) => continue,
                    Err(()) => break,
                }
            }
        });
        Some(Self {
            jobs,
            lifetime,
            token: token.clone(),
        })
    }

    pub(super) fn send(&self, job: Job) -> bool {
        self.jobs.send(job).is_ok()
    }

    pub(super) fn belongs_to(&self, client: &Client) -> bool {
        !client.abandoned && client.token.as_ref() == Some(&self.token)
    }

    pub(super) fn owns(&self, reply: &Finished) -> bool {
        reply.visit.ptr_eq(&Arc::downgrade(&self.lifetime))
    }
}

pub(in crate::app::shell_wol::live_service) struct Finished {
    visit: Weak<()>,
    active: Weak<()>,
    transfer: u64,
    after: u64,
    result: WorkResult,
}

enum WorkResult {
    Ready(Response),
    Download(Result<Response, RequestFailure>),
    Upload {
        completed: usize,
        batch: Result<Value, RequestFailure>,
    },
}

fn resource_name(resource: TransferResource) -> &'static str {
    if resource == TransferResource::Preview {
        "preview"
    } else {
        "map"
    }
}

fn downloaded_chunk(transfer: u64, value: Value) -> Response {
    #[derive(serde::Deserialize)]
    struct Chunk {
        digest: String,
        byte_length: usize,
        offset: usize,
        data: String,
        complete: bool,
    }
    // The room's transfer checks this chunk's digest, size and position.
    serde_json::from_value::<Chunk>(value)
        .ok()
        .and_then(|chunk| {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(chunk.data)
                .ok()?;
            Some(Response::MapChunk {
                transfer,
                digest: chunk.digest,
                total: chunk.byte_length,
                offset: chunk.offset,
                bytes,
                complete: chunk.complete,
            })
        })
        .unwrap_or(Response::MapFailure {
            transfer,
            retryable: false,
        })
}

macro_rules! map_driver {
    ($($asynchronous:tt)?) => {
        impl Client {
            $($asynchronous)? fn map_job(&self, job: Job, visit: &Weak<()>) {
                if job.active.strong_count() == 0
                    || visit.strong_count() == 0
                    || self.stopping.load(Ordering::Acquire)
                {
                    return;
                }
                let Job {
                    transfer,
                    after,
                    io,
                    active,
                    completed: replies,
                } = job;
                let result = match io {
                    MapIo::ReadMapUpload {
                        source,
                        digest,
                        total,
                    } => WorkResult::Ready(match source.read(&digest, total) {
                        Ok(bytes) => Response::MapUploadPrepared { transfer, bytes },
                        Err(_) => Response::MapFailure {
                            transfer,
                            retryable: false,
                        },
                    }),
                    MapIo::CachedMap {
                        resource,
                        directory,
                        digest,
                        total,
                    } => {
                        let loaded =
                            crate::map::source::load_received_asset(&directory, &digest, total, resource)
                                .ok();
                        WorkResult::Ready(Response::CachedMap { transfer, loaded })
                    }
                    MapIo::InstallMap {
                        resource,
                        directory,
                        digest,
                        total,
                        bytes,
                    } => WorkResult::Ready(
                        match crate::map::source::install_received_asset(
                            &directory, bytes, &digest, total, resource,
                        ) {
                            Ok(loaded) => Response::MapInstalled { transfer, loaded },
                            Err(_) => Response::MapFailure {
                                transfer,
                                retryable: false,
                            },
                        },
                    ),
                    MapIo::MapChunk {
                        resource,
                        seed_digest,
                        digest,
                        offset,
                        match_id,
                    } => {
                        let answer = if crate::map::source::valid_source_sha256(&digest) {
                            let watched = match_id.map_or_else(String::new, |id| format!("&match_id={id}"));
                            let path = format!(
                                "/v1/maps/{seed_digest}?offset={offset}&resource={}{watched}",
                                resource_name(resource)
                            );
                            platform_await!(self.request(&path, None))
                                .map(|value| downloaded_chunk(transfer, value))
                        } else {
                            Err(RequestFailure::Status(400))
                        };
                        WorkResult::Download(answer)
                    }
                    MapIo::UploadMapChunk {
                        resource,
                        id,
                        digest,
                        total,
                        offset,
                        bytes,
                    } => {
                        let completed = offset + bytes.len();
                        let body = json!({"request_id": id.to_string(), "after": after, "operation":"map_upload",
                                    "payload": {"digest": digest, "byte_length": total, "offset": offset,
                                        "resource": resource_name(resource), "data": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)}});
                        let batch = platform_await!(self.request("/v1/commands", Some(&body)));
                        WorkResult::Upload { completed, batch }
                    }
                };
                if visit.strong_count() != 0 && !self.stopping.load(Ordering::Acquire) {
                    // Wake the control worker immediately, including on a fast local transfer.
                    let _ = replies.send(Request::MapFinished(Finished {
                        visit: visit.clone(),
                        active,
                        transfer,
                        after,
                        result,
                    }));
                }
            }

            pub(super) $($asynchronous)? fn finish_map(&mut self, reply: Finished, output: &Sender<Response>) {
                let Finished {
                    transfer,
                    active,
                    after,
                    result,
                    ..
                } = reply;
                if active.strong_count() == 0 {
                    // A superseded selection never changes the current transfer. A successful
                    // host command can still carry room events for this same login.
                    if let WorkResult::Upload {
                        batch: Ok(batch), ..
                    } = result
                    {
                        let _ = self.overlapping_batch(batch, output);
                    }
                    return;
                }
                let response = match result {
                    WorkResult::Ready(response) | WorkResult::Download(Ok(response)) => response,
                    WorkResult::Download(Err(error)) => {
                        platform_await!(self.session_failed(error.clone(), output));
                        match error {
                            RequestFailure::Status(409 | 429) => Response::MapPending { transfer },
                            _ => Response::MapFailure {
                                transfer,
                                retryable: matches!(error.status(), 0 | 500..=599),
                            },
                        }
                    }
                    WorkResult::Upload { completed, batch } => {
                        match batch.and_then(|value| self.overlapping_batch(value, output)) {
                            Ok(()) => Response::MapUploaded {
                                transfer,
                                completed,
                            },
                            Err(RequestFailure::LostCursor) if after < self.cursor => {
                                // The upload used a cursor that the control stream has already recovered.
                                Response::MapFailure {
                                    transfer,
                                    retryable: true,
                                }
                            }
                            Err(error) => {
                                platform_await!(self.session_failed(error.clone(), output));
                                Response::MapFailure {
                                    transfer,
                                    retryable: matches!(
                                        error,
                                        RequestFailure::Status(0 | 409 | 429 | 500..=599)
                                    ),
                                }
                            }
                        }
                    }
                };
                let _ = output.send(response);
            }
        }
    };
}
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
map_driver!();
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
map_driver!(async);

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_maps_upload_still_delivers_room_events_without_advancing_the_new_transfer() {
        let mut client = Client::new(
            Arc::new(AtomicBool::new(false)),
            ureq::Agent::new_with_defaults(),
            String::new(),
        );
        let (output, received) = mpsc::channel();
        let batch = json!({"cursor":1, "events":[{"sequence":1, "kind":"options","payload":{}}]});
        client.finish_map(
            Finished {
                visit: Weak::new(),
                active: Weak::new(),
                transfer: 3,
                after: 0,
                result: WorkResult::Upload {
                    completed: 8,
                    batch: Ok(batch),
                },
            },
            &output,
        );
        assert!(
            matches!(received.try_recv(), Ok(Response::Event(event)) if event.kind == "options")
        );
        assert!(
            received.try_recv().is_err(),
            "the expired transfer delivered a completion"
        );
        assert_eq!(client.cursor, 1);
    }
}
