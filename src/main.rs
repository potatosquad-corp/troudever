use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

enum ProxyEvent {
    Log(String),
    Status(String),
    RoomCode(String),
    Stopped,
}

struct TrouDeVerApp {
    // Configuration
    ws_url: String,
    tcp_addr: String,

    // State
    is_running: bool,
    room_number: Option<String>,
    status_msg: String,
    logs: Vec<String>,

    // Communication
    rx_event: Receiver<ProxyEvent>,
    tx_event: Sender<ProxyEvent>,
    proxy_abort: Option<tokio::task::AbortHandle>,
}

impl Default for TrouDeVerApp {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            ws_url: "ws://localhost:4455".to_owned(),
            tcp_addr: "127.0.0.1:9000".to_owned(),
            is_running: false,
            room_number: None,
            status_msg: "Ready".to_owned(),
            logs: vec![],
            rx_event: rx,
            tx_event: tx,
            proxy_abort: None,
        }
    }
}

impl eframe::App for TrouDeVerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle network events
        while let Ok(event) = self.rx_event.try_recv() {
            match event {
                ProxyEvent::Log(msg) => {
                    let time = chrono::Local::now().format("%H:%M:%S");
                    self.logs.push(format!("[{}] {}", time, msg));
                    if self.logs.len() > 50 {
                        self.logs.remove(0);
                    }
                }
                ProxyEvent::RoomCode(code) => self.room_number = Some(code),
                ProxyEvent::Status(msg) => self.status_msg = msg,
                ProxyEvent::Stopped => {
                    self.is_running = false;
                    self.status_msg = "Stopped".to_string();
                    self.proxy_abort = None;
                }
            }
        }

        // Draw UI
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("TrouDeVer - Proxy");
            ui.separator();

            ui.add_enabled_ui(!self.is_running, |ui| {
                ui.horizontal(|ui| {
                    ui.label("WebSocket URL:");
                    ui.text_edit_singleline(&mut self.ws_url);
                });
                ui.horizontal(|ui| {
                    ui.label("TCP Server:");
                    ui.text_edit_singleline(&mut self.tcp_addr);
                });
            });

            ui.add_space(10.0);

            if self.is_running {
                if ui.button("[ STOP ]").clicked() {
                    self.stop_proxy();
                }
            } else {
                if ui.button("[ CONNECT ]").clicked() {
                    self.start_proxy();
                }
            }

            ui.label(format!("Status: {}", self.status_msg));
            if let Some(code) = &self.room_number {
                ui.add_space(10.0);
                ui.heading(format!("ROOM CODE : {}", code));
                if ui.button("Copier").clicked() {
                    ui.ctx().copy_text(code.to_string());
                }
            }
            ui.separator();

            ui.heading("Logs");
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for log in &self.logs {
                        ui.label(log);
                    }
                });
        });

        ctx.request_repaint();
    }
}

impl TrouDeVerApp {
    fn start_proxy(&mut self) {
        self.is_running = true;
        self.status_msg = "Starting...".to_string();
        self.logs.clear();

        let ws_url = self.ws_url.clone();
        let tcp_addr = self.tcp_addr.clone();
        let tx = self.tx_event.clone();

        let handle = tokio::spawn(async move {
            run_proxy_logic(ws_url, tcp_addr, tx.clone()).await;
            let _ = tx.send(ProxyEvent::Stopped);
        });

        self.proxy_abort = Some(handle.abort_handle());
    }

    fn stop_proxy(&mut self) {
        if let Some(handle) = &self.proxy_abort {
            handle.abort();
            self.logs.push("Stopped by user.".to_string());
        }
        self.proxy_abort = None;
        self.is_running = false;
        self.status_msg = "Stopping...".to_string();
    }
}

async fn run_proxy_logic(ws_url: String, tcp_addr: String, tx: Sender<ProxyEvent>) {
    let _ = tx.send(ProxyEvent::Log(format!(
        "Connecting to WebSocket at {}...",
        ws_url
    )));

    let url = match Url::parse(&ws_url) {
        Ok(u) => u,
        Err(e) => {
            let _ = tx.send(ProxyEvent::Log(format!("Invalid URL: {}", e)));
            return;
        }
    };

    let ws_stream = match connect_async(url.as_str()).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            let _ = tx.send(ProxyEvent::Log(format!(
                "WebSocket connection failed: {}",
                e
            )));
            return;
        }
    };
    let _ = tx.send(ProxyEvent::Log("[OK] WebSocket Connected".to_string()));

    let _ = tx.send(ProxyEvent::Log(format!(
        "Connecting to TCP Server at {}...",
        tcp_addr
    )));
    let tcp_stream = match TcpStream::connect(&tcp_addr).await {
        Ok(s) => {
            if let Err(e) = s.set_nodelay(true) {
                let _ = tx.send(ProxyEvent::Log(format!(
                    "Warning: Failed to set TCP_NODELAY: {}",
                    e
                )));
            }
            s
        }
        Err(e) => {
            let _ = tx.send(ProxyEvent::Log(format!("TCP connection failed: {}", e)));
            return;
        }
    };
    let _ = tx.send(ProxyEvent::Log(
        "[OK] TCP Connected. Tunnel active.".to_string(),
    ));
    let _ = tx.send(ProxyEvent::Status("Connected (Active)".to_string()));

    let (mut ws_write, mut ws_read) = ws_stream.split();
    let (mut tcp_read, mut tcp_write) = tcp_stream.into_split();
    let mut tcp_buffer = vec![0u8; 1_048_576];

    loop {
        tokio::select! {
            // WebSocket -> TCP
            Some(msg) = ws_read.next() => {
                match msg {
                    Ok(message) => {
                        let data = message.into_data();
                        if !data.is_empty() {
                            let len = data.len() as u32;
                            let len_bytes = len.to_be_bytes();

                            if let Err(e) = tcp_write.write_all(&len_bytes).await {
                                let _ = tx.send(ProxyEvent::Log(format!("TCP write len error: {}", e)));
                                break;
                            }

                            if let Err(e) = tcp_write.write_all(&data).await {
                                let _ = tx.send(ProxyEvent::Log(format!("TCP write data error: {}", e)));
                                break;
                            }

                            let _ = tcp_write.flush().await;
                        }
                    },
                    Err(e) => {
                        let _ = tx.send(ProxyEvent::Log(format!("WebSocket read error: {}", e)));
                        break;
                    }
                }
            }

            // TCP -> WebSocket
            result = tcp_read.read(&mut tcp_buffer) => {
                match result {
                    Ok(0) => {
                        let _ = tx.send(ProxyEvent::Log("TCP server closed connection".to_string()));
                        break;
                    }
                    Ok(n) => {
                        let data_chunk = &tcp_buffer[0..n];
                        let stream = serde_json::Deserializer::from_slice(&data_chunk).into_iter::<Value>();
                        let mut forward_message = true;
                        for json in stream {
                            if let Ok(value) = json {
                                if let Some(_) = value.get("internal") {
                                    forward_message = false;
                                    if let Some(code) = value.get("room").and_then(|v| v.as_str()){
                                        let _ = tx.send(ProxyEvent::RoomCode(code.to_string()));
                                        let _ = tx.send(ProxyEvent::Log(format!("Room ID reçue: {}", code)));
                                    }
                                }
                            }
                        }
                        if forward_message {
                            let ws_message = match std::str::from_utf8(data_chunk) {
                                Ok(text) => Message::Text(text.to_string().into()),
                                Err(_) => Message::Binary(data_chunk.to_vec().into()),
                            };

                            if let Err(e) = ws_write.send(ws_message).await {
                                let _ = tx.send(ProxyEvent::Log(format!("WebSocket send error: {}", e)));
                                break;
                            }
                            let _ = ws_write.flush();
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(ProxyEvent::Log(format!("TCP read error: {}", e)));
                        break;
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([400.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "TrouDeVer",
        options,
        Box::new(|_cc| Ok(Box::new(TrouDeVerApp::default()))),
    )
}
