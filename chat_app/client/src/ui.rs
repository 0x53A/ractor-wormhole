use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use ractor::ActorRef;
use ractor_wormhole::util::{ActorRef_Ask, ActorRef_FilterMap as _, FnActor};
use shared::{ChatMessage, ChatServerMessage, UserAlias};

use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

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
                        if let Some(server) = state.server.clone() {
                            if !state.composer.is_empty() {
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
        // Create the main layout
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title area
                Constraint::Min(5),    // Chat history area
                Constraint::Length(3), // Composer area
            ])
            .margin(1)
            .split(frame.area());

        // Create outer border
        let outer_block = Block::default().borders(Borders::ALL);
        frame.render_widget(outer_block, frame.area());

        // Title area with username
        let title = format!(
            "You are: {}",
            self.user_alias
                .clone()
                .map(|u| u.to_string())
                .unwrap_or("connecting".to_string())
        );
        let title_block = Block::default()
            .borders(Borders::ALL)
            .title_alignment(ratatui::layout::Alignment::Center)
            .title(title);
        frame.render_widget(title_block, main_layout[0]);

        // Chat history area
        self.render_chat_history(frame, main_layout[1]);

        // Composer area (split into input and button)
        let composer_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(20),   // Input takes most space
                Constraint::Length(8), // Send button is fixed width
            ])
            .split(main_layout[2]);

        // Input area
        let input_text = if self.is_message_in_flight {
            "Sending..."
        } else {
            &self.composer
        };
        let input_block = Block::default().borders(Borders::ALL);
        let input = Paragraph::new(input_text).block(input_block);
        frame.render_widget(input, composer_layout[0]);

        // Send button
        let button_block = Block::default().borders(Borders::ALL).title("Send >");
        frame.render_widget(button_block, composer_layout[1]);
    }

    fn render_chat_history(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        if self.chat_history.is_empty() {
            return;
        }

        // Calculate how many messages we can display
        // Each message takes approximately 2 lines (name + spacing)
        let max_messages = (area.height / 2) as usize;
        let start_idx = if self.chat_history.len() > max_messages {
            self.chat_history.len() - max_messages
        } else {
            0
        };

        for (i, idx) in (start_idx..self.chat_history.len()).enumerate() {
            let y_position = area.y + (i as u16 * 2);

            match &self.chat_history[idx] {
                ChatEntry::Message(user_alias, message) => {
                    // Create username box
                    let name_len = user_alias.to_string().len() as u16;
                    let name_box_width = name_len + 2; // Add border space
                    let name_area = ratatui::layout::Rect {
                        x: area.x,
                        y: y_position,
                        width: name_box_width,
                        height: 1,
                    };

                    // Render username box
                    let name_block = Block::default().borders(Borders::ALL);
                    frame.render_widget(name_block, name_area);

                    // Render username inside box
                    let name_text = Paragraph::new(user_alias.to_string().clone());
                    frame.render_widget(
                        name_text,
                        ratatui::layout::Rect {
                            x: name_area.x + 1,
                            y: name_area.y,
                            width: name_len,
                            height: 1,
                        },
                    );

                    // Render message content
                    let message_area = ratatui::layout::Rect {
                        x: name_area.x + name_box_width + 2, // Space after name
                        y: y_position,
                        width: area.width - name_box_width - 2,
                        height: 1,
                    };
                    let message_text = Paragraph::new(message.0.clone());
                    frame.render_widget(message_text, message_area);
                }
                ChatEntry::UserConnected(user_alias) => {
                    // Simple connected message
                    let text = format!("{} connected", user_alias.to_string());
                    let connected_text =
                        Paragraph::new(text).style(Style::default().fg(Color::Green));

                    let message_area = ratatui::layout::Rect {
                        x: area.x,
                        y: y_position,
                        width: area.width,
                        height: 1,
                    };
                    frame.render_widget(connected_text, message_area);
                }
            }
        }
    }
}
