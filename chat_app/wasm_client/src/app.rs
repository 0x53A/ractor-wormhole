use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::util::{ActorRef_Ask, FnActor};
use shared::{ChatClientMessage, ChatMessage, ChatServerMessage, UserAlias};
use std::sync::mpsc;

#[derive(Debug)]
pub enum UiUpdate {
    Connected(UserAlias, ActorRef<ChatServerMessage>),
    UserConnected(UserAlias),
    MessageReceived(UserAlias, ChatMessage),
    Disconnected,
    Error(String),
}

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
    #[serde(skip)] // Don't persist chat messages or connection state
    messages: Vec<(String, String)>,
    #[serde(skip)]
    input_message: String,
    #[serde(skip)]
    user_alias: Option<String>,
    #[serde(skip)]
    status: String,

    #[serde(skip)]
    chat_server_ref: Option<ActorRef<ChatServerMessage>>,
    #[serde(skip)]
    ui_update_rx: Option<mpsc::Receiver<UiUpdate>>,
    #[serde(skip)]
    portal_ref: Option<ActorRef<ractor_wormhole::portal::PortalActorMessage>>,
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
            portal_ref: None,
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
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(16, 18, 22);
        visuals.window_fill = egui::Color32::from_rgb(20, 23, 28);
        visuals.extreme_bg_color = egui::Color32::from_rgb(11, 13, 16);
        visuals.hyperlink_color = egui::Color32::from_rgb(125, 190, 255);
        visuals.selection.bg_fill = egui::Color32::from_rgb(42, 106, 140);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            ui_update_rx: Some(ui_update_rx),
            portal_ref: Some(portal_ref),
            status: "Joining chat...".to_owned(),
            ..Default::default()
        }
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
                            format!("{} joined.", alias.to_string()),
                        ));
                    }
                    UiUpdate::MessageReceived(alias, msg) => {
                        self.messages.push((alias.to_string(), msg.to_string()));
                    }
                    UiUpdate::Disconnected => {
                        self.status = "Disconnected.".to_owned();
                        self.chat_server_ref = None;
                        self.messages
                            .push(("System".to_string(), "Disconnected.".to_string()));
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_events();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("Ractor Chat")
                        .size(24.0)
                        .color(egui::Color32::from_rgb(235, 240, 246)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let connected = self.chat_server_ref.is_some();
                    let dot = if connected {
                        egui::Color32::from_rgb(65, 210, 125)
                    } else {
                        egui::Color32::from_rgb(225, 172, 72)
                    };
                    ui.colored_label(dot, "●");
                    ui.label(egui::RichText::new(&self.status).color(egui::Color32::GRAY));
                });
            });

            if let Some(alias) = &self.user_alias {
                ui.label(
                    egui::RichText::new(format!("Signed in as {alias}"))
                        .color(egui::Color32::from_rgb(150, 170, 190)),
                );
            }

            ui.separator();

            let composer_height = 44.0;
            let chat_height = (ui.available_height() - composer_height).max(120.0);

            ui.allocate_ui(egui::vec2(ui.available_width(), chat_height), |ui| {
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(12, 14, 18))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height((chat_height - 24.0).max(80.0))
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.messages.is_empty() {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            egui::RichText::new("Connecting to the chat room")
                                                .color(egui::Color32::from_rgb(140, 150, 165)),
                                        );
                                    });
                                }

                                for (alias, msg) in &self.messages {
                                    let is_system = alias == "System";
                                    let is_self = self.user_alias.as_ref() == Some(alias);
                                    let fill = if is_system {
                                        egui::Color32::from_rgb(26, 30, 36)
                                    } else if is_self {
                                        egui::Color32::from_rgb(28, 86, 118)
                                    } else {
                                        egui::Color32::from_rgb(32, 36, 44)
                                    };

                                    ui.horizontal_wrapped(|ui| {
                                        egui::Frame::default()
                                            .fill(fill)
                                            .inner_margin(egui::Margin::symmetric(10, 7))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    egui::RichText::new(alias).strong().color(
                                                        egui::Color32::from_rgb(190, 210, 225),
                                                    ),
                                                );
                                                ui.label(
                                                    egui::RichText::new(msg).color(
                                                        egui::Color32::from_rgb(235, 238, 242),
                                                    ),
                                                );
                                            });
                                    });
                                }
                            });
                    });
            });

            ui.horizontal(|ui| {
                let send_width = 72.0;
                let input_response = ui.add_sized(
                    [(ui.available_width() - send_width - 8.0).max(120.0), 34.0],
                    egui::TextEdit::singleline(&mut self.input_message)
                        .hint_text("Message")
                        .margin(egui::vec2(10.0, 7.0)),
                );

                let send_button = ui.add_sized(
                    [send_width, 34.0],
                    egui::Button::new(egui::RichText::new("Send").strong()),
                );

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
                        input_response.request_focus();
                    } else {
                        self.messages
                            .push(("System".to_string(), "Not connected.".to_string()));
                    }
                }
            });
        });
    }
}
