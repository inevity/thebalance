use worker::{event, Env, Result, Stub, MessageExt};
use crate::state::strategy::ApiKeyStatus;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum StateUpdate {
    SetStatus {
        key_id: String,
        status: ApiKeyStatus,
    },
    SetCooldown {
        key_id: String,
        model: String,
        duration_secs: u64,
    },
}

// Helper to get the Durable Object stub for the API Key Manager.
pub(crate) fn get_do_stub(env: &Env) -> Result<Stub> {
    let namespace = env.durable_object("API_KEY_MANAGER")?;
    namespace.id_from_name("v1")?.get_stub()
}

// Helper to call the "set status" endpoint on the Durable Object.
pub(crate) async fn set_key_status(key_id: &str, status: ApiKeyStatus, env: &Env) -> Result<()> {
    let do_stub = get_do_stub(env)?;
    let mut req_init = worker::RequestInit::new();
    req_init.with_method(worker::Method::Put);
    let body = serde_json::to_string(&serde_json::json!({ "status": status }))?;
    let req = worker::Request::new_with_init(
        &format!("https://fake-host/keys/{}/status", key_id),
        &req_init.with_body(Some(body.into())),
    )?;
    do_stub.fetch_with_request(req).await?;
    Ok(())
}

// Helper to call the "set cooldown" endpoint on the Durable Object.
pub(crate) async fn set_key_cooldown(key_id: &str, model: &str, duration_secs: u64, env: &Env) -> Result<()> {
    let do_stub = get_do_stub(env)?;
    let mut req_init = worker::RequestInit::new();
    req_init.with_method(worker::Method::Post);
    let body = serde_json::to_string(&serde_json::json!({ "model": model, "duration_secs": duration_secs }))?;
    let req = worker::Request::new_with_init(
        &format!("https://fake-host/keys/{}/cooldown", key_id),
        &req_init.with_body(Some(body.into())),
    )?;
    do_stub.fetch_with_request(req).await?;
    Ok(())
}

#[event(queue)]
pub async fn main(batch: worker::MessageBatch<StateUpdate>, env: Env, _ctx: worker::Context) -> Result<()> {
    #[cfg(feature = "raw_d1")]
    let db = env.d1("DB")?;

    for message in batch.messages()? {
        worker::console_log!("Processing state update: {:?}", message.body());
        let res = match message.body() {
            StateUpdate::SetStatus { key_id, status } => {
                #[cfg(feature = "raw_d1")]
                { crate::d1_storage::update_status(&db, &key_id, status.clone()).await }
                #[cfg(not(feature = "raw_d1"))]
                { set_key_status(&key_id, status.clone(), &env).await }
            }
            StateUpdate::SetCooldown { key_id, model, duration_secs } => {
                #[cfg(feature = "raw_d1")]
                { crate::d1_storage::set_cooldown(&db, &key_id, &model, *duration_secs).await }
                #[cfg(not(feature = "raw_d1"))]
                { set_key_cooldown(&key_id, &model, *duration_secs, &env).await }
            }
        };

        if let Err(e) = res {
            worker::console_error!("Failed to process state update {:?}: {}", message.body(), e);
            message.retry();
        } else {
            message.ack();
        }
    }
    Ok(())
}
