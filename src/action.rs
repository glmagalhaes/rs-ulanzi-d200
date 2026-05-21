use anyhow::Result;
use async_trait::async_trait;
use log::info;
use openaction::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CycleStatusWindowSettings {
    pub current: u8,
}

pub struct CycleStatusWindow {
    pub cycle_tx: mpsc::Sender<()>,
}

#[async_trait]
impl Action for CycleStatusWindow {
    const UUID: ActionUuid = "com.gitlab.glmagalhaes.opendeck-ulanzi-d200.cycle";
    type Settings = CycleStatusWindowSettings;

    async fn key_up(&self, _instance: &Instance, _settings: &Self::Settings) -> OpenActionResult<()> {
        info!("Cycling display mode");
        let _ = self.cycle_tx.send(()).await;
        Ok(())
    }

    async fn will_appear(&self, _: &Instance, _: &Self::Settings) -> OpenActionResult<()> {
        Ok(())
    }
}