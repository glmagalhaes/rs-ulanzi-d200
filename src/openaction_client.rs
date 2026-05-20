use log::info;
use openaction::async_trait;
use openaction::global_events::{
    set_global_event_handler, DeviceDidConnectEvent, DeviceDidDisconnectEvent, GlobalEventHandler,
    SetBrightnessEvent, SetImageEvent,
};
use openaction::OpenActionResult;
use tokio::sync::mpsc;

#[allow(dead_code)]
#[derive(Debug)]
pub enum BridgeEvent {
    SetImage {
        device_id: String,
        position: u8,
        image_base64: String,
    },
    ClearImage {
        device_id: String,
        position: u8,
    },
    SetBrightness {
        device_id: String,
        brightness: u8,
    },
    #[allow(dead_code)]
    DeviceConnected(String),
    DeviceDisconnected(String),
}

pub struct OpenActionBridge {
    pub tx: mpsc::Sender<BridgeEvent>,
}

impl OpenActionBridge {
    pub fn new(tx: mpsc::Sender<BridgeEvent>) -> Self {
        Self { tx }
    }

    pub fn register(self) {
        let leaked = Box::leak(Box::new(self));
        set_global_event_handler(leaked);
    }
}

#[async_trait]
impl GlobalEventHandler for OpenActionBridge {
    async fn plugin_ready(&self) -> OpenActionResult<()> {
        info!("OpenAction Bridge: Plugin Ready");
        Ok(())
    }

    async fn device_plugin_set_image(&self, event: SetImageEvent) -> OpenActionResult<()> {
        if let Some(pos) = event.position {
            let device_id = event.device.clone();
            if let Some(img) = event.image {
                let _ = self
                    .tx
                    .send(BridgeEvent::SetImage {
                        device_id,
                        position: pos,
                        image_base64: img,
                    })
                    .await;
            } else {
                let _ = self
                    .tx
                    .send(BridgeEvent::ClearImage {
                        device_id,
                        position: pos,
                    })
                    .await;
            }
        }
        Ok(())
    }

    async fn device_plugin_set_brightness(
        &self,
        event: SetBrightnessEvent,
    ) -> OpenActionResult<()> {
        let _ = self
            .tx
            .send(BridgeEvent::SetBrightness {
                device_id: event.device.clone(),
                brightness: event.brightness,
            })
            .await;
        Ok(())
    }

    async fn device_did_connect(&self, event: DeviceDidConnectEvent) -> OpenActionResult<()> {
        info!("Device Connected: {}", event.device);
        let _ = self
            .tx
            .send(BridgeEvent::DeviceConnected(event.device))
            .await;
        Ok(())
    }

    async fn device_did_disconnect(&self, event: DeviceDidDisconnectEvent) -> OpenActionResult<()> {
        info!("Device Disconnected: {}", event.device);
        let _ = self
            .tx
            .send(BridgeEvent::DeviceDisconnected(event.device))
            .await;
        Ok(())
    }
}
