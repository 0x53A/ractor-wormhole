use futures::SinkExt;
use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::util::{ActorRef_Ask, FnActor};
use shared::{ChatClientMessage, ChatMessage, ChatServerMessage, UserAlias};
use std::sync::mpsc;

pub enum ChatEntry {
    Message(UserAlias, ChatMessage),
    UserConnected(UserAlias),
}

pub struct ConnectedUIState {
    pub alias: UserAlias,
    pub server: ActorRef<ChatServerMessage>,
    pub chat_history: Vec<ChatEntry>,
    pub is_message_in_flight: bool,
    pub composer: String,
}

pub enum UIState {
    Connecting,
    Connected,
}

// Add this enum
#[derive(Debug)]
pub enum UiUpdate {
    // Received initial connection info from server
    Connected(
        UserAlias,
        ActorRef<ChatServerMessage>, // Direct ref to server actor
                                     // We might need the portal ref too if the server ref isn't directly usable via wormhole
                                     // portal_ref: ActorRef<ractor_wormhole::portal::PortalActorMessage>,
    ),
    // Another user connected
    UserConnected(UserAlias),
    // A message was received
    MessageReceived(UserAlias, ChatMessage),
    // We were disconnected
    Disconnected,
    // An error occurred
    Error(String),
}

// Helper function to start the actor
pub async fn start_client_handler_actor(
    ui_tx: std::sync::mpsc::Sender<UiUpdate>,
    request_repaint: tokio::sync::mpsc::Sender<()>,
) -> Result<ActorRef<ChatClientMessage>, anyhow::Error> {
    let (actor_ref, _handle) = FnActor::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            log::info!("ClientMessageHandlerActor received: {msg:?}");
            let update_msg = match msg {
                ChatClientMessage::UserConnected(alias) => UiUpdate::UserConnected(alias),
                ChatClientMessage::MessageReceived(alias, msg) => {
                    UiUpdate::MessageReceived(alias, msg)
                }
                ChatClientMessage::Disconnect => UiUpdate::Disconnected,
            };

            if let Err(e) = ui_tx.send(update_msg) {
                log::error!("Failed to send UI update: {e}");
            }
            request_repaint.send(()).await.unwrap();
        }
    })
    .await?;

    Ok(actor_ref)
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct TemplateApp {
    // Chat state:
    #[serde(skip)] // Don't persist chat messages or connection state
    messages: Vec<(String, String)>, // List of (alias, message)
    #[serde(skip)]
    input_message: String,
    #[serde(skip)]
    user_alias: Option<String>,
    #[serde(skip)]
    status: String,

    // Actor / Communication handles:
    #[serde(skip)]
    chat_server_ref: Option<ActorRef<ChatServerMessage>>, // Ref to send messages TO server
    #[serde(skip)]
    ui_update_rx: Option<mpsc::Receiver<UiUpdate>>, // Channel to receive updates FROM handler actor
    #[serde(skip)]
    portal_ref: Option<ActorRef<ractor_wormhole::portal::PortalActorMessage>>, // Ref to the wormhole portal

    // Example stuff (can be removed or kept):
    label: String,
    value: f32,
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input_message: String::new(),
            user_alias: None,
            status: "Connecting...".to_owned(),
            chat_server_ref: None,
            ui_update_rx: None,
            portal_ref: None, // Initialize portal_ref
            // Example stuff:
            label: "Chat Input".to_owned(), // Change default label
            value: 0.0,                     // Reset value
        }
    }
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        portal_ref: ActorRef<ractor_wormhole::portal::PortalActorMessage>, // Receive portal ref
        ui_update_rx: mpsc::Receiver<UiUpdate>, // Receive channel receiver
    ) -> Self {
        // Customize egui visuals/fonts if needed
        // cc.egui_ctx.set_visuals(egui::Visuals::dark());

        // Basic state initialization
        let mut app = Self {
            ui_update_rx: Some(ui_update_rx),
            portal_ref: Some(portal_ref), // Store portal ref
            status: "Initializing connection...".to_owned(),
            ..Default::default()
        };

        // Load previous app state (if any) - Note: Chat state is skipped
        if let Some(storage) = cc.storage {
            if let Some(loaded_app) = eframe::get_value::<Self>(storage, eframe::APP_KEY) {
                // Keep loaded persistent fields, but overwrite transient state
                app.label = loaded_app.label;
                app.value = loaded_app.value;
                // Keep the ui_update_rx and portal_ref we just received
            }
        }

        app // Return the initialized app
    }
}

impl TemplateApp {
    /// handles all outstanding (currently queued) events
    fn handle_events(&mut self) {
        // Process any pending UI updates from the receiver channel
        if let Some(rx) = &self.ui_update_rx {
            while let Ok(update) = rx.try_recv() {
                log::debug!("UI received update: {update:?}");
                match update {
                    UiUpdate::Connected(alias, server_ref) => {
                        self.user_alias = Some(alias.to_string());
                        self.chat_server_ref = Some(server_ref);
                        self.status = format!("Connected as {}", self.user_alias.as_ref().unwrap());
                        self.messages.push((
                            "System".to_string(),
                            format!("Connected as {}", self.user_alias.as_ref().unwrap()),
                        ));
                    }
                    UiUpdate::UserConnected(alias) => {
                        self.messages.push((
                            "System".to_string(),
                            format!("User '{}' connected.", alias.to_string()),
                        ));
                    }
                    UiUpdate::MessageReceived(alias, msg) => {
                        self.messages.push((alias.to_string(), msg.to_string()));
                    }
                    UiUpdate::Disconnected => {
                        self.status = "Disconnected.".to_owned();
                        self.chat_server_ref = None; // Clear server ref on disconnect
                        self.messages
                            .push(("System".to_string(), "Disconnected.".to_string()));
                        // Optionally clear other state or attempt reconnect
                    }
                    UiUpdate::Error(err_msg) => {
                        self.status = format!("Error: {err_msg}");
                        self.messages
                            .push(("System".to_string(), format!("Error: {err_msg}")));
                    }
                }
            }
        }
    }
}

impl eframe::App for TemplateApp {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();

        // Top Panel for menu (optional)
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            // TODO: Consider sending a disconnect message before closing
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }
                egui::widgets::global_theme_preference_buttons(ui);
                ui.separator();
                // Display connection status and alias
                if let Some(alias) = &self.user_alias {
                    ui.label(format!("Alias: {alias}"));
                }
                ui.label(&self.status);
            });
        });

        // Central Panel for Chat
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Ractor Chat");

            // Message display area
            ui.separator();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true) // Keep scrolled to the bottom
                .show(ui, |ui| {
                    for (alias, msg) in &self.messages {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("{alias}:"));
                            ui.label(msg);
                        });
                    }
                });
            ui.separator();

            // Input area
            ui.horizontal(|ui| {
                let input_response = ui.add_sized(
                    [
                        ui.available_width() - 50.0,
                        ui.text_style_height(&egui::TextStyle::Body),
                    ], // Adjust size as needed
                    egui::TextEdit::singleline(&mut self.input_message)
                        .hint_text("Enter message..."),
                );

                let send_button = ui.button("Send");

                // Send message on button click or Enter key press in text edit
                if (send_button.clicked()
                    || (input_response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    && !self.input_message.trim().is_empty()
                {
                    if let Some(server_ref) = &self.chat_server_ref {
                        let msg_to_send = ChatMessage(self.input_message.clone());
                        log::info!("Sending message: {}", msg_to_send.0);
                        let _ = server_ref.ask_then(
                            |rpc| ChatServerMessage::PostMessage(msg_to_send, rpc),
                            Some(Duration::from_secs(10)),
                            |r| match r {
                                Ok(_) => log::info!("Message sent successfully"),
                                Err(e) => log::error!("Failed to send message: {e}"),
                            },
                        );

                        self.input_message.clear();
                        input_response.request_focus(); // Keep focus on input after sending
                    } else {
                        self.messages
                            .push(("System".to_string(), "Not connected.".to_string()));
                    }
                }
            });
        });
    }
}
