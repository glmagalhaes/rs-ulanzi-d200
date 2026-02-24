use futures_util::StreamExt;
use openaction::{device_plugin, run, OpenActionResult};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;

#[tokio::main]
async fn main() -> OpenActionResult<()> {
    // 1. Start a dummy WS server
    let listener = TcpListener::bind("127.0.0.1:57117").await.unwrap();
    println!("Mock server listening on 57117");

    // Spawn server task
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let mut ws_stream = accept_async(stream).await.expect("Failed to accept");
            println!("Mock server: Connection accepted");
            while let Some(msg) = ws_stream.next().await {
                if let Ok(msg) = msg {
                    println!("Mock server received: {:?}", msg);
                }
            }
        }
    });

    // 2. Setup args for openaction to connect to our mock server
    let args = vec![
        "program".to_string(),
        "-port".to_string(),
        "57117".to_string(),
        "-pluginUUID".to_string(),
        "MY_UUID".to_string(),
        "-registerEvent".to_string(),
        "register".to_string(),
        "-info".to_string(),
        "{}".to_string(),
    ];

    // 3. Spawn "Device Hardware" logic
    tokio::spawn(async move {
        // Wait a bit for connection to be established
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("Calling register_device...");
        // fn(id: String, name: String, rows: u8, cols: u8, type: u8, encoder_count: u8)
        let _ = device_plugin::register_device(
            "dev1".to_string(),
            "Ulanzi D200".to_string(),
            3,
            5,
            7,
            0,
        )
        .await;

        println!("Calling key_down...");
        let _ = device_plugin::key_down("dev1".to_string(), 0).await;

        // Force exit after test
        tokio::time::sleep(Duration::from_secs(1)).await;
        std::process::exit(0);
    });

    println!("Running openaction loop...");
    run(args).await?;

    Ok(())
}
