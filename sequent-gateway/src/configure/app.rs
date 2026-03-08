//! TUI application state and input handling.

use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::config::{BoardInstance, GatewayConfig, I2cConfig, LoggingConfig, ServerConfig};
use super::AvailableBoard;

// ════════════════════════════════════════════════════════════════════════
// Screens
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    BoardSelect,
    BoardConfig,
    ServerSettings,
    I2cSettings,
    Review,
}

// ════════════════════════════════════════════════════════════════════════
// App state
// ════════════════════════════════════════════════════════════════════════

/// Selected board with its per-instance configuration.
#[derive(Debug, Clone)]
pub struct SelectedBoard {
    pub slug: String,
    pub display_name: String,
    #[allow(dead_code)]
    pub capabilities: String,
    pub stack: u8,
    pub slave_id: u8,
}

/// An editable text field with cursor.
#[derive(Debug, Clone)]
pub struct EditField {
    pub label: &'static str,
    pub value: String,
    pub editing: bool,
}

impl EditField {
    pub fn new(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
            editing: false,
        }
    }
}

/// Complete TUI application state.
pub struct App {
    /// Current screen.
    pub screen: Screen,

    /// All discovered boards.
    pub available: Vec<AvailableBoard>,

    /// Cursor position in the board list.
    pub board_cursor: usize,

    /// Boards the user has selected (toggled).
    pub selected_boards: Vec<SelectedBoard>,

    /// Cursor in the board-config screen.
    pub config_cursor: usize,

    /// Which selected-board is being configured.
    pub config_board_idx: usize,

    /// Server settings fields.
    pub server_fields: Vec<EditField>,
    /// Server settings cursor.
    pub server_cursor: usize,

    /// I²C settings fields.
    pub i2c_fields: Vec<EditField>,
    /// I²C settings cursor.
    pub i2c_cursor: usize,

    /// Output file path.
    pub output_path: PathBuf,

    /// Whether the config was saved.
    pub saved: bool,

    /// Scroll offset for the board list.
    #[allow(dead_code)]
    pub board_scroll: usize,

    /// Status message (shown at the bottom).
    pub status: String,

    /// Whether an edit field is currently focused for text input.
    pub editing: bool,

    /// Generated TOML preview (populated on review screen).
    pub preview: String,

    /// Scroll offset for the review screen.
    pub review_scroll: usize,
}

impl App {
    pub fn new(
        available: Vec<AvailableBoard>,
        existing: Option<GatewayConfig>,
        output_path: PathBuf,
    ) -> Self {
        let cfg = existing.unwrap_or_default();

        // Pre-select boards from existing config
        let mut selected_boards = Vec::new();
        for bi in &cfg.board {
            if let Some(ab) = available.iter().find(|a| a.slug == bi.board_type) {
                selected_boards.push(SelectedBoard {
                    slug: ab.slug.clone(),
                    display_name: ab.display_name.clone(),
                    capabilities: ab.capabilities.clone(),
                    stack: bi.stack,
                    slave_id: bi.slave_id,
                });
            }
        }

        let server_fields = vec![
            EditField::new("Host", &cfg.server.host),
            EditField::new("Modbus Port", cfg.server.port.to_string()),
            EditField::new(
                "Health Port",
                cfg.server
                    .health_port
                    .map_or(String::new(), |p| p.to_string()),
            ),
            EditField::new(
                "Single Slave",
                if cfg.server.single_slave { "yes" } else { "no" },
            ),
        ];

        let i2c_fields = vec![
            EditField::new("Reset Threshold", cfg.i2c.reset_threshold.to_string()),
            EditField::new(
                "Channel Fault Threshold",
                cfg.i2c.channel_fault_threshold.to_string(),
            ),
            EditField::new(
                "Relay Verify Interval",
                cfg.i2c.relay_verify_interval.to_string(),
            ),
            EditField::new("Log Interval (s)", cfg.logging.interval.to_string()),
            EditField::new(
                "Log File",
                cfg.logging
                    .file
                    .as_ref()
                    .map_or(String::new(), |p| p.to_string_lossy().into()),
            ),
            EditField::new("Log Retention", cfg.logging.retention.to_string()),
            EditField::new(
                "Map Opto to Reg",
                if cfg.map_opto_to_reg { "yes" } else { "no" },
            ),
        ];

        Self {
            screen: Screen::BoardSelect,
            available,
            board_cursor: 0,
            selected_boards,
            config_cursor: 0,
            config_board_idx: 0,
            server_fields,
            server_cursor: 0,
            i2c_fields,
            i2c_cursor: 0,
            output_path,
            saved: false,
            board_scroll: 0,
            status: "Select boards with Space, Enter to continue".into(),
            editing: false,
            preview: String::new(),
            review_scroll: 0,
        }
    }

    /// Navigate back one screen.
    pub fn go_back(&mut self) {
        self.editing = false;
        self.screen = match self.screen {
            Screen::BoardSelect => Screen::BoardSelect,
            Screen::BoardConfig => Screen::BoardSelect,
            Screen::ServerSettings => Screen::BoardConfig,
            Screen::I2cSettings => Screen::ServerSettings,
            Screen::Review => Screen::I2cSettings,
        };
        self.update_status();
    }

    /// Handle a keypress. Returns `true` if the app should exit.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        // If we're editing a text field, route to field input handler
        if self.editing {
            return self.handle_edit_key(key);
        }

        match self.screen {
            Screen::BoardSelect => self.handle_board_select(key),
            Screen::BoardConfig => self.handle_board_config(key),
            Screen::ServerSettings => self.handle_server_settings(key),
            Screen::I2cSettings => self.handle_i2c_settings(key),
            Screen::Review => self.handle_review(key),
        }
    }

    // ── Board selection screen ───────────────────────────────────────

    fn handle_board_select(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.board_cursor > 0 {
                    self.board_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.board_cursor + 1 < self.available.len() {
                    self.board_cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                self.toggle_board();
            }
            KeyCode::Enter => {
                if self.selected_boards.is_empty() {
                    self.status = "⚠ Select at least one board before continuing".into();
                } else {
                    self.config_board_idx = 0;
                    self.config_cursor = 0;
                    self.screen = Screen::BoardConfig;
                    self.update_status();
                }
            }
            _ => {}
        }
        false
    }

    fn toggle_board(&mut self) {
        let slug = &self.available[self.board_cursor].slug;
        if let Some(pos) = self.selected_boards.iter().position(|s| &s.slug == slug) {
            self.selected_boards.remove(pos);
        } else {
            let ab = &self.available[self.board_cursor];
            self.selected_boards.push(SelectedBoard {
                slug: ab.slug.clone(),
                display_name: ab.display_name.clone(),
                capabilities: ab.capabilities.clone(),
                stack: 0,
                slave_id: (self.selected_boards.len() as u8 + 1).min(247),
            });
        }
    }

    // ── Board config screen ──────────────────────────────────────────

    fn handle_board_config(&mut self, key: KeyCode) -> bool {
        let board_count = self.selected_boards.len();
        // Each board has 2 fields (stack, slave_id) + a "Next board" option
        let fields_per_board = 2;
        let total_items = fields_per_board + 1; // +1 for the [Continue] button

        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.config_cursor > 0 {
                    self.config_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.config_cursor + 1 < total_items {
                    self.config_cursor += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.config_board_idx > 0 {
                    self.config_board_idx -= 1;
                    self.config_cursor = 0;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.config_board_idx + 1 < board_count {
                    self.config_board_idx += 1;
                    self.config_cursor = 0;
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.adjust_board_field(1);
            }
            KeyCode::Char('-') => {
                self.adjust_board_field(-1);
            }
            KeyCode::Enter => {
                if self.config_cursor == fields_per_board {
                    // Continue button
                    self.screen = Screen::ServerSettings;
                    self.server_cursor = 0;
                    self.update_status();
                }
            }
            KeyCode::Tab => {
                // Quick jump to next board
                if self.config_board_idx + 1 < board_count {
                    self.config_board_idx += 1;
                    self.config_cursor = 0;
                }
            }
            _ => {}
        }
        false
    }

    fn adjust_board_field(&mut self, delta: i8) {
        if self.config_board_idx >= self.selected_boards.len() {
            return;
        }
        let board = &mut self.selected_boards[self.config_board_idx];
        match self.config_cursor {
            0 => {
                // Stack ID [0–7]
                let new_val = board.stack as i16 + delta as i16;
                board.stack = new_val.clamp(0, 7) as u8;
            }
            1 => {
                // Slave ID [1–247]
                let new_val = board.slave_id as i16 + delta as i16;
                board.slave_id = new_val.clamp(1, 247) as u8;
            }
            _ => {}
        }
    }

    // ── Server settings screen ───────────────────────────────────────

    fn handle_server_settings(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.server_cursor > 0 {
                    self.server_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.server_cursor + 1 < self.server_fields.len() + 1 {
                    self.server_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if self.server_cursor < self.server_fields.len() {
                    let field = &mut self.server_fields[self.server_cursor];
                    // Toggle yes/no fields
                    if field.label == "Single Slave" {
                        field.value = if field.value == "yes" {
                            "no".into()
                        } else {
                            "yes".into()
                        };
                    } else {
                        field.editing = true;
                        self.editing = true;
                        self.status = "Type value, Enter to confirm, Esc to cancel".into();
                    }
                } else {
                    // Continue button
                    self.screen = Screen::I2cSettings;
                    self.i2c_cursor = 0;
                    self.update_status();
                }
            }
            _ => {}
        }
        false
    }

    // ── I²C settings screen ─────────────────────────────────────────

    fn handle_i2c_settings(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.i2c_cursor > 0 {
                    self.i2c_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.i2c_cursor + 1 < self.i2c_fields.len() + 1 {
                    self.i2c_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if self.i2c_cursor < self.i2c_fields.len() {
                    let field = &mut self.i2c_fields[self.i2c_cursor];
                    // Toggle yes/no fields
                    if field.label == "Map Opto to Reg" {
                        field.value = if field.value == "yes" {
                            "no".into()
                        } else {
                            "yes".into()
                        };
                    } else {
                        field.editing = true;
                        self.editing = true;
                        self.status = "Type value, Enter to confirm, Esc to cancel".into();
                    }
                } else {
                    // Continue → Review
                    self.build_preview();
                    self.review_scroll = 0;
                    self.screen = Screen::Review;
                    self.update_status();
                }
            }
            _ => {}
        }
        false
    }

    // ── Edit field input ─────────────────────────────────────────────

    fn handle_edit_key(&mut self, key: KeyCode) -> bool {
        let fields = match self.screen {
            Screen::ServerSettings => &mut self.server_fields,
            Screen::I2cSettings => &mut self.i2c_fields,
            _ => return false,
        };
        let cursor = match self.screen {
            Screen::ServerSettings => self.server_cursor,
            Screen::I2cSettings => self.i2c_cursor,
            _ => return false,
        };

        if cursor >= fields.len() {
            return false;
        }

        let field = &mut fields[cursor];

        match key {
            KeyCode::Enter => {
                field.editing = false;
                self.editing = false;
                self.update_status();
            }
            KeyCode::Esc => {
                field.editing = false;
                self.editing = false;
                self.update_status();
            }
            KeyCode::Backspace => {
                field.value.pop();
            }
            KeyCode::Char(c) => {
                field.value.push(c);
            }
            _ => {}
        }
        false
    }

    // ── Review screen ────────────────────────────────────────────────

    fn handle_review(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.review_scroll > 0 {
                    self.review_scroll -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.review_scroll += 1;
            }
            KeyCode::Char('s') | KeyCode::Enter => {
                // Save
                let cfg = self.build_config();
                match cfg.save(&self.output_path) {
                    Ok(()) => {
                        self.saved = true;
                        return true;
                    }
                    Err(e) => {
                        self.status = format!("❌ Save failed: {e}");
                    }
                }
            }
            _ => {}
        }
        false
    }

    // ── Config builder ───────────────────────────────────────────────

    pub fn build_config(&self) -> GatewayConfig {
        let host = self.server_fields[0].value.clone();
        let port = self.server_fields[1].value.parse().unwrap_or(502);
        let health_port = self.server_fields[2].value.parse().ok();
        let single_slave = self.server_fields[3].value == "yes";

        let reset_threshold = self.i2c_fields[0].value.parse().unwrap_or(10);
        let channel_fault = self.i2c_fields[1].value.parse().unwrap_or(5);
        let relay_verify = self.i2c_fields[2].value.parse().unwrap_or(10);
        let log_interval = self.i2c_fields[3].value.parse().unwrap_or(5);
        let log_file = if self.i2c_fields[4].value.is_empty() {
            None
        } else {
            Some(PathBuf::from(&self.i2c_fields[4].value))
        };
        let log_retention = self.i2c_fields[5].value.parse().unwrap_or(7);
        let map_opto = self.i2c_fields[6].value == "yes";

        let boards: Vec<BoardInstance> = self
            .selected_boards
            .iter()
            .map(|sb| BoardInstance {
                board_type: sb.slug.clone(),
                stack: sb.stack,
                slave_id: sb.slave_id,
            })
            .collect();

        GatewayConfig {
            server: ServerConfig {
                host,
                port,
                health_port,
                single_slave,
            },
            logging: LoggingConfig {
                interval: log_interval,
                file: log_file,
                retention: log_retention,
            },
            i2c: I2cConfig {
                reset_threshold,
                channel_fault_threshold: channel_fault,
                relay_verify_interval: relay_verify,
            },
            board: boards,
            boards_dir: PathBuf::from("boards"),
            map_opto_to_reg: map_opto,
            builtin_defaults: false,
        }
    }

    fn build_preview(&mut self) {
        let cfg = self.build_config();
        self.preview = toml::to_string_pretty(&cfg).unwrap_or_else(|e| format!("Error: {e}"));
    }

    fn update_status(&mut self) {
        self.status = match self.screen {
            Screen::BoardSelect => "↑↓ Navigate  Space Toggle  Enter Continue  q/Esc Quit".into(),
            Screen::BoardConfig => "↑↓ Select field  +/- Adjust  ←→/Tab Switch board  Enter Continue  Esc Back".into(),
            Screen::ServerSettings => "↑↓ Navigate  Enter Edit/Toggle  Esc Back".into(),
            Screen::I2cSettings => "↑↓ Navigate  Enter Edit/Toggle  Esc Back".into(),
            Screen::Review => "↑↓ Scroll  s/Enter Save  Esc Back".into(),
        };
    }
}
