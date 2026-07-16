use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use ractor::ActorRef;
use ractor_wormhole::util::{ActorRef_Ask, ActorRef_FilterMap as _, FnActor};
use shared::{ChatMessage, ChatServerMessage, UserAlias};

use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

mod theme {
    use ratatui::style::Color;

    pub const ACCENT: Color = Color::Rgb(255, 94, 0);
    pub const ACCENT_BRIGHT: Color = Color::Rgb(255, 140, 50);
    pub const BG_PANEL: Color = Color::Rgb(19, 19, 22);
    pub const BG_WINDOW: Color = Color::Rgb(14, 14, 16);
    pub const BG_WIDGET: Color = Color::Rgb(28, 28, 32);
    pub const BG_DEEP: Color = Color::Rgb(9, 9, 10);

    pub const STROKE_DIM: Color = Color::Rgb(52, 52, 58);
    pub const TEXT: Color = Color::Rgb(222, 222, 216);
    pub const TEXT_DIM: Color = Color::Rgb(140, 140, 134);
}

pub enum ChatEntry {
    Message(UserAlias, ChatMessage),
    UserConnected(UserAlias),
}

pub struct UIState {
    pub user_alias: Option<UserAlias>,
    pub server: Option<ActorRef<ChatServerMessage>>,
    pub chat_history: Vec<ChatEntry>,
    pub is_message_in_flight: bool,
    pub composer: String,

    pub exit: bool,
}

impl UIState {
    pub fn new() -> Self {
        Self {
            user_alias: None,
            server: None,
            chat_history: Vec::new(),
            is_message_in_flight: false,
            composer: String::new(),
            exit: false,
        }
    }
}

pub enum UIMsg {
    Connected(UserAlias, ActorRef<ChatServerMessage>),

    AddChatMessage(UserAlias, ChatMessage),
    /// a different user connected
    UserConnected(UserAlias),
    Disconnected,
    SetMessageInFlight(bool),

    InputEvent(KeyEvent),
}

async fn event_reader_loop(actor_ref: ActorRef<Event>) {
    let mut reader = crossterm::event::EventStream::new();
    loop {
        let evt = reader.next().await;
        if let Some(Ok(evt)) = evt {
            if actor_ref.send_message(evt).is_err() {
                break;
            }
        } else {
            break;
        }
    }
}

pub async fn spawn_ui_actor<T: Backend + Send + 'static>(
    mut terminal: Terminal<T>,
) -> ActorRef<UIMsg> {
    let (actor_ref, _) = FnActor::<UIMsg>::start_fn(async move |mut ctx| {
        let mut state = UIState::new();

        // this receives a message of type crossterm::Event and forwards it to this actor
        let (key_input_event_receiver, _) = ctx
            .actor_ref
            .clone()
            .filter_map(|evt| {
                if let Event::Key(key) = evt
                    && key.kind == crossterm::event::KeyEventKind::Press
                {
                    Some(UIMsg::InputEvent(key))
                } else {
                    None
                }
            })
            .await
            .unwrap();

        tokio::spawn(event_reader_loop(key_input_event_receiver));

        // draw the initial UI
        terminal.draw(|frame| state.ui(frame)).unwrap();

        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                UIMsg::Connected(user_alias, server) => {
                    state.user_alias = Some(user_alias);
                    state.server = Some(server);
                }
                UIMsg::AddChatMessage(user_alias, chat_message) => {
                    state
                        .chat_history
                        .push(ChatEntry::Message(user_alias, chat_message));
                }
                UIMsg::SetMessageInFlight(is_in_flight) => {
                    state.is_message_in_flight = is_in_flight;
                }
                UIMsg::Disconnected => {
                    state.exit = true;
                }
                UIMsg::InputEvent(key) => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // kill actor on CTRL+C
                        state.exit = true;
                    }
                    KeyCode::Esc => {
                        state.composer.clear();
                    }
                    KeyCode::Enter => {
                        if let Some(server) = state.server.clone()
                            && !state.composer.is_empty()
                        {
                            state.is_message_in_flight = true;

                            let msg_to_send = state.composer.clone();
                            state.composer.clear();
                            let self_copy = ctx.actor_ref.clone();
                            server
                                .ask_then(
                                    |rpc| {
                                        ChatServerMessage::PostMessage(
                                            ChatMessage(msg_to_send),
                                            rpc,
                                        )
                                    },
                                    None,
                                    move |_| {
                                        self_copy
                                            .send_message(UIMsg::SetMessageInFlight(false))
                                            .unwrap();
                                    },
                                )
                                .unwrap();
                        }
                    }

                    KeyCode::Char(c) => {
                        // add character to composer
                        state.composer.push(c);
                    }
                    KeyCode::Backspace => {
                        // remove last character from composer
                        state.composer.pop();
                    }
                    _ => {}
                },
                UIMsg::UserConnected(user_alias) => {
                    state
                        .chat_history
                        .push(ChatEntry::UserConnected(user_alias));
                }
            }

            if state.exit {
                // exit the UI
                return;
            }

            // after processing any event, redraw
            terminal.draw(|frame| state.ui(frame)).unwrap();
        }
    })
    .await
    .unwrap();

    actor_ref
}

impl UIState {
    fn ui(&self, frame: &mut Frame) {
        let canvas = Block::default().style(Style::default().bg(theme::BG_PANEL));
        frame.render_widget(canvas, frame.area());

        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(5),
                Constraint::Length(4),
                Constraint::Length(1),
            ])
            .margin(1)
            .split(frame.area());

        self.render_header(frame, main_layout[0]);
        self.render_chat_history(frame, main_layout[1]);
        self.render_composer(frame, main_layout[2]);
        self.render_status(frame, main_layout[3]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let connected = self.server.is_some();
        let alias = self
            .user_alias
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "NO ALIAS".to_string());
        let state = if connected { "ONLINE" } else { "LINKING" };

        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("  ", Style::default().bg(theme::ACCENT)),
                Span::raw(" "),
                Span::styled(
                    "RACTOR CHAT",
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  // wormhole console",
                    Style::default().fg(theme::TEXT_DIM),
                ),
            ]),
            Line::from(vec![
                Span::styled("  ALIAS ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(alias, Style::default().fg(theme::ACCENT_BRIGHT)),
                Span::styled("  LINK ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(
                    state,
                    Style::default().fg(if connected {
                        theme::ACCENT
                    } else {
                        theme::TEXT_DIM
                    }),
                ),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::STROKE_DIM))
                .style(Style::default().bg(theme::BG_DEEP)),
        )
        .style(Style::default().bg(theme::BG_DEEP));

        frame.render_widget(header, area);

        if area.height > 0 {
            let rule = Rect {
                x: area.x + 1,
                y: area.y + area.height.saturating_sub(1),
                width: area.width.saturating_sub(2),
                height: 1,
            };
            frame.render_widget(
                Paragraph::new("─".repeat(rule.width as usize))
                    .style(Style::default().fg(theme::ACCENT).bg(theme::BG_DEEP)),
                rule,
            );
        }
    }

    fn render_chat_history(&self, frame: &mut Frame, area: Rect) {
        let inner_height = area.height.saturating_sub(2).max(1) as usize;
        let start_idx = self.chat_history.len().saturating_sub(inner_height);
        let mut lines = Vec::new();

        if self.chat_history.is_empty() {
            lines.push(Line::from(Span::styled(
                "-- awaiting link --",
                Style::default().fg(theme::TEXT_DIM),
            )));
        } else {
            for entry in &self.chat_history[start_idx..] {
                match entry {
                    ChatEntry::Message(user_alias, message) => {
                        let alias = user_alias.to_string();
                        let is_self = self.user_alias.as_ref().map(ToString::to_string).as_deref()
                            == Some(alias.as_str());
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{:>12}", alias.to_uppercase()),
                                Style::default().fg(if is_self {
                                    theme::ACCENT_BRIGHT
                                } else {
                                    theme::TEXT_DIM
                                }),
                            ),
                            Span::styled(" │ ", Style::default().fg(theme::STROKE_DIM)),
                            Span::styled(message.0.clone(), Style::default().fg(theme::TEXT)),
                        ]));
                    }
                    ChatEntry::UserConnected(user_alias) => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "        JOIN",
                                Style::default()
                                    .fg(theme::ACCENT)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" │ ", Style::default().fg(theme::STROKE_DIM)),
                            Span::styled(
                                format!("{user_alias} connected"),
                                Style::default().fg(theme::TEXT_DIM),
                            ),
                        ]));
                    }
                }
            }
        }

        let traffic = Paragraph::new(lines)
            .block(Self::section_block("TRAFFIC").style(Style::default().bg(theme::BG_WINDOW)))
            .style(Style::default().bg(theme::BG_WINDOW))
            .wrap(Wrap { trim: false });

        frame.render_widget(traffic, area);
    }

    fn render_composer(&self, frame: &mut Frame, area: Rect) {
        let composer_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(12)])
            .split(area);

        let input_text = if self.is_message_in_flight {
            "SENDING..."
        } else if self.composer.is_empty() {
            "MESSAGE"
        } else {
            &self.composer
        };
        let input_style = if self.composer.is_empty() || self.is_message_in_flight {
            Style::default().fg(theme::TEXT_DIM).bg(theme::BG_WIDGET)
        } else {
            Style::default().fg(theme::TEXT).bg(theme::BG_WIDGET)
        };

        let input = Paragraph::new(input_text)
            .block(Self::section_block("TRANSMIT").style(Style::default().bg(theme::BG_WIDGET)))
            .style(input_style);
        frame.render_widget(input, composer_layout[0]);

        let send_style = if self.server.is_some() && !self.is_message_in_flight {
            Style::default()
                .fg(theme::ACCENT_BRIGHT)
                .bg(theme::BG_WIDGET)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_DIM).bg(theme::BG_WIDGET)
        };

        let button = Paragraph::new("SEND")
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if self.server.is_some() {
                        theme::ACCENT
                    } else {
                        theme::STROKE_DIM
                    }))
                    .style(Style::default().bg(theme::BG_WIDGET)),
            )
            .style(send_style);
        frame.render_widget(button, composer_layout[1]);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let status = if self.server.is_some() {
            "ONLINE"
        } else {
            "NO SERVER"
        };
        let text = Line::from(vec![
            Span::styled(status, Style::default().fg(theme::ACCENT)),
            Span::styled("  MSG ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(
                self.chat_history.len().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ),
            Span::styled(
                "  CTRL+C EXIT  ESC CLEAR",
                Style::default().fg(theme::TEXT_DIM),
            ),
        ]);
        let status_bar =
            Paragraph::new(text).style(Style::default().fg(theme::TEXT_DIM).bg(theme::BG_DEEP));
        frame.render_widget(status_bar, area);
    }

    fn section_block(title: &'static str) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::STROKE_DIM))
            .title(Line::from(vec![
                Span::styled(" ", Style::default().bg(theme::ACCENT)),
                Span::styled(
                    format!(" {title} "),
                    Style::default()
                        .fg(theme::ACCENT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
    }
}
