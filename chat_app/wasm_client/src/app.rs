use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::util::{ActorRef_Ask, FnActor};
use shared::{ChatClientMessage, ChatMessage, ChatServerMessage, UserAlias};
use std::sync::mpsc;

use egui::{CornerRadius, RichText, Stroke};

mod theme {
    use egui::{
        Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Theme, Vec2, style::Selection,
    };

    pub const ACCENT: Color32 = Color32::from_rgb(255, 94, 0);
    pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(255, 140, 50);
    pub const ERROR: Color32 = Color32::from_rgb(255, 70, 54);

    pub const BG_WINDOW: Color32 = Color32::from_rgb(14, 14, 16);
    pub const BG_PANEL: Color32 = Color32::from_rgb(19, 19, 22);
    pub const BG_WIDGET: Color32 = Color32::from_rgb(28, 28, 32);
    pub const BG_HOVER: Color32 = Color32::from_rgb(38, 38, 43);
    pub const BG_DEEP: Color32 = Color32::from_rgb(9, 9, 10);

    pub const STROKE_DIM: Color32 = Color32::from_rgb(52, 52, 58);
    pub const TEXT: Color32 = Color32::from_rgb(222, 222, 216);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 140, 134);

    pub fn apply(ctx: &egui::Context) {
        ctx.set_theme(Theme::Dark);
        ctx.style_mut_of(Theme::Dark, |style| {
            style.text_styles = [
                (TextStyle::Heading, FontId::new(16.0, FontFamily::Monospace)),
                (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
                (
                    TextStyle::Monospace,
                    FontId::new(12.5, FontFamily::Monospace),
                ),
                (TextStyle::Button, FontId::new(13.0, FontFamily::Monospace)),
                (TextStyle::Small, FontId::new(10.5, FontFamily::Monospace)),
            ]
            .into();

            style.spacing.item_spacing = Vec2::new(8.0, 6.0);
            style.spacing.button_padding = Vec2::new(14.0, 6.0);

            let v = &mut style.visuals;
            v.dark_mode = true;
            v.window_fill = BG_WINDOW;
            v.panel_fill = BG_PANEL;
            v.extreme_bg_color = BG_DEEP;
            v.code_bg_color = BG_DEEP;
            v.faint_bg_color = BG_WIDGET;
            v.warn_fg_color = ACCENT_BRIGHT;
            v.error_fg_color = ERROR;
            v.hyperlink_color = ACCENT_BRIGHT;
            v.selection = Selection {
                bg_fill: ACCENT.gamma_multiply(0.55),
                stroke: Stroke::new(1.0, Color32::WHITE),
            };

            v.window_corner_radius = CornerRadius::ZERO;
            v.menu_corner_radius = CornerRadius::ZERO;
            let w = &mut v.widgets;
            for wv in [
                &mut w.noninteractive,
                &mut w.inactive,
                &mut w.hovered,
                &mut w.active,
                &mut w.open,
            ] {
                wv.corner_radius = CornerRadius::ZERO;
            }

            w.noninteractive.bg_fill = BG_PANEL;
            w.noninteractive.bg_stroke = Stroke::new(1.0, STROKE_DIM);
            w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

            w.inactive.bg_fill = BG_WIDGET;
            w.inactive.weak_bg_fill = BG_WIDGET;
            w.inactive.bg_stroke = Stroke::new(1.0, STROKE_DIM);
            w.inactive.fg_stroke = Stroke::new(1.0, TEXT);

            w.hovered.bg_fill = BG_HOVER;
            w.hovered.weak_bg_fill = BG_HOVER;
            w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
            w.hovered.fg_stroke = Stroke::new(1.5, ACCENT_BRIGHT);

            w.active.bg_fill = ACCENT;
            w.active.weak_bg_fill = ACCENT;
            w.active.bg_stroke = Stroke::new(1.0, ACCENT);
            w.active.fg_stroke = Stroke::new(1.5, Color32::BLACK);

            w.open.bg_fill = BG_WIDGET;
            w.open.weak_bg_fill = BG_WIDGET;
            w.open.bg_stroke = Stroke::new(1.0, ACCENT);
            w.open.fg_stroke = Stroke::new(1.0, ACCENT_BRIGHT);

            v.interact_cursor = Some(egui::CursorIcon::PointingHand);
        });
    }
}

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
        theme::apply(&cc.egui_ctx);

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
                        self.messages
                            .push(("System".to_string(), format!("{} joined.", alias)));
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

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_events();

        self.header_bar(ui);
        self.status_bar(ui);
        self.composer_bar(ui);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_PANEL))
            .show(ui, |ui| {
                ui.add_space(12.0);
                Self::section(ui, "TRAFFIC", |ui| {
                    self.message_well(ui);
                });
            });
    }
}

impl TemplateApp {
    fn header_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::top("chat_header")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DEEP)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (mark, _) =
                        ui.allocate_exact_size(egui::Vec2::new(10.0, 22.0), egui::Sense::hover());
                    ui.painter().rect_filled(mark, 0.0, theme::ACCENT);
                    ui.heading(RichText::new("RACTOR CHAT").color(theme::TEXT).strong());
                    ui.label(
                        RichText::new("// wormhole console")
                            .monospace()
                            .color(theme::TEXT_DIM),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let connected = self.chat_server_ref.is_some();
                        ui.label(
                            RichText::new(if connected { "ONLINE" } else { "LINKING" })
                                .small()
                                .color(if connected {
                                    theme::ACCENT
                                } else {
                                    theme::TEXT_DIM
                                }),
                        );
                    });
                });

                let (line, _) = ui.allocate_exact_size(
                    egui::Vec2::new(ui.available_width(), 2.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(line, 0.0, theme::ACCENT);
            });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::bottom("chat_status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DEEP)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).small().color(
                        if self.status.starts_with("Error") {
                            theme::ERROR
                        } else {
                            theme::TEXT_DIM
                        },
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} MSG", self.messages.len()))
                                .small()
                                .color(theme::ACCENT_BRIGHT),
                        );
                        let alias = self.user_alias.as_deref().unwrap_or("NO ALIAS");
                        ui.label(RichText::new(alias).small().color(theme::TEXT_DIM));
                    });
                });
            });
    }

    fn composer_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("chat_composer")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show_separator_line(false)
            .show(ui, |ui| {
                Self::section(ui, "TRANSMIT", |ui| {
                    self.composer(ui);
                });
            });
    }

    fn section<R>(
        ui: &mut egui::Ui,
        title: &str,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        ui.horizontal(|ui| {
            let (tick, _) =
                ui.allocate_exact_size(egui::Vec2::new(4.0, 12.0), egui::Sense::hover());
            ui.painter().rect_filled(tick, 0.0, theme::ACCENT);
            ui.label(RichText::new(title).small().color(theme::ACCENT_BRIGHT));
        });

        egui::Frame::new()
            .fill(theme::BG_WINDOW)
            .stroke(Stroke::new(1.0, theme::STROKE_DIM))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                add_contents(ui)
            })
            .inner
    }

    fn message_well(&self, ui: &mut egui::Ui) {
        let well_height = ui.available_height().max(120.0);
        egui::Frame::new()
            .fill(theme::BG_DEEP)
            .stroke(Stroke::new(1.0, theme::STROKE_DIM))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_height(well_height);
                egui::ScrollArea::vertical()
                    .id_salt("chat_scroll")
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.messages.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.label(
                                    RichText::new("-- awaiting link --")
                                        .monospace()
                                        .color(theme::TEXT_DIM),
                                );
                            });
                        }

                        for (alias, msg) in &self.messages {
                            self.message_row(ui, alias, msg);
                        }
                    });
            });
    }

    fn message_row(&self, ui: &mut egui::Ui, alias: &str, msg: &str) {
        let is_system = alias == "System";
        let is_self = self.user_alias.as_deref() == Some(alias);
        let stroke = if is_self {
            Stroke::new(1.0, theme::ACCENT)
        } else {
            Stroke::new(1.0, theme::STROKE_DIM)
        };
        let fill = if is_system {
            theme::BG_WINDOW
        } else {
            theme::BG_WIDGET
        };
        let text_color = if is_system {
            theme::TEXT_DIM
        } else {
            theme::TEXT
        };

        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized(
                        [92.0, 18.0],
                        egui::Label::new(
                            RichText::new(alias.to_uppercase())
                                .monospace()
                                .small()
                                .color(if is_self {
                                    theme::ACCENT_BRIGHT
                                } else {
                                    theme::TEXT_DIM
                                }),
                        ),
                    );
                    ui.label(RichText::new(msg).color(text_color));
                });
            });
    }

    fn composer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let send_width = 92.0;
            let input_response = ui.add_sized(
                [(ui.available_width() - send_width - 8.0).max(120.0), 34.0],
                egui::TextEdit::singleline(&mut self.input_message)
                    .hint_text("MESSAGE")
                    .margin(egui::vec2(10.0, 7.0)),
            );

            let send_button = ui.add_enabled_ui(self.chat_server_ref.is_some(), |ui| {
                ui.add_sized(
                    [send_width, 34.0],
                    egui::Button::new(RichText::new("SEND").strong()),
                )
            });

            if (send_button.inner.clicked()
                || (input_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                && !self.input_message.trim().is_empty()
            {
                self.send_current_message();
                input_response.request_focus();
            }
        });
    }

    fn send_current_message(&mut self) {
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
        } else {
            self.messages
                .push(("System".to_string(), "Not connected.".to_string()));
        }
    }
}
