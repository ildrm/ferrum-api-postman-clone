//! Original native desktop experience for Ferrum API.

use std::{path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextStyle};
use ferrum_app_services::{AppSnapshot, ExecutionOutput, FerrumService};
use ferrum_domain::{
    Collection, Environment, EnvironmentId, HistoryEntry, HttpMethod, HttpResponse, KeyValue,
    RequestBody, RequestId, SavedRequest, Variable,
};
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

/// Semantic design tokens for one appearance mode.
#[derive(Clone, Copy, Debug)]
struct Palette {
    accent: Color32,
    accent_soft: Color32,
    panel: Color32,
    canvas: Color32,
    elevated: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    success: Color32,
    warning: Color32,
    danger: Color32,
}

impl Palette {
    fn dark() -> Self {
        Self {
            accent: Color32::from_rgb(255, 153, 78),
            accent_soft: Color32::from_rgb(62, 45, 35),
            panel: Color32::from_rgb(28, 32, 39),
            canvas: Color32::from_rgb(18, 21, 27),
            elevated: Color32::from_rgb(36, 41, 49),
            border: Color32::from_rgb(66, 74, 86),
            text: Color32::from_rgb(232, 235, 240),
            muted: Color32::from_rgb(174, 182, 194),
            success: Color32::from_rgb(68, 202, 178),
            warning: Color32::from_rgb(247, 194, 85),
            danger: Color32::from_rgb(246, 119, 119),
        }
    }

    fn light() -> Self {
        Self {
            accent: Color32::from_rgb(190, 82, 20),
            accent_soft: Color32::from_rgb(252, 231, 217),
            panel: Color32::from_rgb(246, 247, 249),
            canvas: Color32::from_rgb(255, 255, 255),
            elevated: Color32::from_rgb(235, 238, 242),
            border: Color32::from_rgb(197, 203, 213),
            text: Color32::from_rgb(31, 36, 44),
            muted: Color32::from_rgb(83, 92, 105),
            success: Color32::from_rgb(14, 124, 102),
            warning: Color32::from_rgb(151, 99, 8),
            danger: Color32::from_rgb(184, 45, 54),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThemeMode {
    System,
    Dark,
    Light,
}

impl ThemeMode {
    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Navigation {
    Collections,
    Environments,
    History,
}

impl Navigation {
    fn label(self) -> &'static str {
        match self {
            Self::Collections => "Collections",
            Self::Environments => "Environments",
            Self::History => "History",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestEditorTab {
    Params,
    Headers,
    Body,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseTab {
    Pretty,
    Raw,
    Headers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryFilter {
    All,
    Success,
    Failed,
}

impl HistoryFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All results",
            Self::Success => "Successful",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoticeKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug)]
struct RequestTab {
    draft: SavedRequest,
    dirty: bool,
    response: Option<HttpResponse>,
}

enum UiMessage {
    Executed {
        request_id: RequestId,
        result: Result<ExecutionOutput, String>,
    },
    RequestSaved {
        request: SavedRequest,
        result: Result<(), String>,
    },
    CollectionCreated(Result<Collection, String>),
    EnvironmentSaved {
        environment: Environment,
        result: Result<(), String>,
    },
}

/// Main eframe application.
#[allow(clippy::struct_excessive_bools)]
pub struct FerrumApp {
    service: Arc<FerrumService>,
    runtime: Handle,
    sender: std::sync::mpsc::Sender<UiMessage>,
    receiver: std::sync::mpsc::Receiver<UiMessage>,
    snapshot: AppSnapshot,
    navigation: Navigation,
    editor_tab: RequestEditorTab,
    response_tab: ResponseTab,
    history_filter: HistoryFilter,
    theme_mode: ThemeMode,
    tabs: Vec<RequestTab>,
    active_tab: usize,
    selected_environment: Option<EnvironmentId>,
    collection_name: String,
    environment_name: String,
    search: String,
    response_search: String,
    status_message: String,
    notice_kind: NoticeKind,
    cancellation: Option<CancellationToken>,
    pending_close: Option<usize>,
    show_collection_dialog: bool,
    show_environment_dialog: bool,
    environment_dirty: bool,
    palette: Palette,
    audit_screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    rendered_frames: u8,
}

impl FerrumApp {
    /// Creates the desktop state from an already-loaded local snapshot.
    pub fn new(
        context: &eframe::CreationContext<'_>,
        service: Arc<FerrumService>,
        runtime: Handle,
        snapshot: AppSnapshot,
    ) -> Self {
        let theme_mode = match std::env::var("FERRUM_AUDIT_THEME").as_deref() {
            Ok("dark") => ThemeMode::Dark,
            Ok("light") => ThemeMode::Light,
            _ => ThemeMode::System,
        };
        let preferred_dark = matches!(
            context
                .egui_ctx
                .system_theme()
                .unwrap_or_else(|| context.egui_ctx.theme()),
            egui::Theme::Dark
        );
        let dark = match theme_mode {
            ThemeMode::System => preferred_dark,
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
        };
        let palette = if dark {
            Palette::dark()
        } else {
            Palette::light()
        };
        configure_style(&context.egui_ctx, palette, dark);
        let (sender, receiver) = std::sync::mpsc::channel();
        let first = snapshot
            .requests
            .first()
            .cloned()
            .unwrap_or_else(|| SavedRequest::blank(snapshot.workspace.id));
        let audit_view = std::env::var("FERRUM_AUDIT_VIEW").ok();
        let navigation = match audit_view.as_deref() {
            Some("environments") => Navigation::Environments,
            Some("history") => Navigation::History,
            _ => Navigation::Collections,
        };
        let selected_environment = audit_view
            .as_deref()
            .filter(|view| *view == "environments")
            .and_then(|_| {
                snapshot
                    .environments
                    .first()
                    .map(|environment| environment.id)
            });
        let audit_response = (audit_view.as_deref() == Some("response")).then(sample_response);
        Self {
            service,
            runtime,
            sender,
            receiver,
            snapshot,
            navigation,
            editor_tab: RequestEditorTab::Params,
            response_tab: ResponseTab::Pretty,
            history_filter: HistoryFilter::All,
            theme_mode,
            tabs: vec![RequestTab {
                draft: first,
                dirty: false,
                response: audit_response,
            }],
            active_tab: 0,
            selected_environment,
            collection_name: String::new(),
            environment_name: String::new(),
            search: String::new(),
            response_search: String::new(),
            status_message: "Ready — local-only workspace".into(),
            notice_kind: NoticeKind::Info,
            cancellation: None,
            pending_close: None,
            show_collection_dialog: false,
            show_environment_dialog: false,
            environment_dirty: false,
            palette,
            audit_screenshot: std::env::var_os("FERRUM_AUDIT_SCREENSHOT").map(PathBuf::from),
            screenshot_requested: false,
            rendered_frames: 0,
        }
    }

    fn active(&self) -> &RequestTab {
        &self.tabs[self.active_tab]
    }

    fn active_mut(&mut self) -> &mut RequestTab {
        &mut self.tabs[self.active_tab]
    }

    fn set_notice(&mut self, kind: NoticeKind, message: impl Into<String>) {
        self.notice_kind = kind;
        self.status_message = message.into();
    }

    fn process_messages(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                UiMessage::Executed { request_id, result } => {
                    self.cancellation = None;
                    if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.draft.id == request_id) {
                        tab.dirty = false;
                        upsert_request(&mut self.snapshot.requests, tab.draft.clone());
                    }
                    match result {
                        Ok(output) => {
                            if let Some(tab) =
                                self.tabs.iter_mut().find(|tab| tab.draft.id == request_id)
                            {
                                tab.response = Some(output.response);
                            }
                            self.snapshot.history.insert(0, output.history);
                            self.set_notice(
                                NoticeKind::Success,
                                "Request completed and saved locally",
                            );
                        }
                        Err(error) if error == "request cancelled" => {
                            self.set_notice(NoticeKind::Warning, "Request cancelled");
                        }
                        Err(error) => self.set_notice(NoticeKind::Error, error),
                    }
                }
                UiMessage::RequestSaved { request, result } => match result {
                    Ok(()) => {
                        upsert_request(&mut self.snapshot.requests, request.clone());
                        if let Some(tab) =
                            self.tabs.iter_mut().find(|tab| tab.draft.id == request.id)
                        {
                            tab.draft = request;
                            tab.dirty = false;
                        }
                        self.set_notice(NoticeKind::Success, "Request saved locally");
                    }
                    Err(error) => self.set_notice(NoticeKind::Error, error),
                },
                UiMessage::CollectionCreated(result) => match result {
                    Ok(collection) => {
                        self.active_mut().draft.collection_id = Some(collection.id);
                        self.active_mut().dirty = true;
                        self.snapshot.collections.push(collection);
                        self.collection_name.clear();
                        self.set_notice(NoticeKind::Success, "Collection created");
                    }
                    Err(error) => self.set_notice(NoticeKind::Error, error),
                },
                UiMessage::EnvironmentSaved {
                    environment,
                    result,
                } => match result {
                    Ok(()) => {
                        if let Some(existing) = self
                            .snapshot
                            .environments
                            .iter_mut()
                            .find(|item| item.id == environment.id)
                        {
                            *existing = environment.clone();
                        } else {
                            self.snapshot.environments.push(environment.clone());
                        }
                        self.selected_environment = Some(environment.id);
                        self.environment_name.clear();
                        self.environment_dirty = false;
                        self.set_notice(NoticeKind::Success, "Environment saved securely");
                    }
                    Err(error) => self.set_notice(NoticeKind::Error, error),
                },
            }
        }
    }

    fn new_request(&mut self) {
        self.tabs.push(RequestTab {
            draft: SavedRequest::blank(self.snapshot.workspace.id),
            dirty: false,
            response: None,
        });
        self.active_tab = self.tabs.len() - 1;
        self.navigation = Navigation::Collections;
        self.editor_tab = RequestEditorTab::Params;
        self.response_tab = ResponseTab::Pretty;
    }

    fn open_request(&mut self, request: SavedRequest) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.draft.id == request.id) {
            self.active_tab = index;
        } else {
            self.tabs.push(RequestTab {
                draft: request,
                dirty: false,
                response: None,
            });
            self.active_tab = self.tabs.len() - 1;
        }
    }

    fn request_close(&mut self, index: usize) {
        if self.tabs[index].dirty {
            self.pending_close = Some(index);
        } else {
            self.close_tab(index);
        }
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() == 1 {
            self.tabs[0] = RequestTab {
                draft: SavedRequest::blank(self.snapshot.workspace.id),
                dirty: false,
                response: None,
            };
            self.active_tab = 0;
            return;
        }
        self.tabs.remove(index);
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
    }

    fn save_active(&mut self, context: &egui::Context) {
        let mut request = self.active().draft.clone();
        if request.name.trim().is_empty() {
            self.set_notice(NoticeKind::Error, "Give the request a name before saving");
            return;
        }
        let untrimmed_name = std::mem::take(&mut request.name);
        untrimmed_name.trim().clone_into(&mut request.name);
        request.updated_at = Utc::now();
        let service = self.service.clone();
        let sender = self.sender.clone();
        let context = context.clone();
        self.runtime.spawn(async move {
            let result = service
                .save_request(&request)
                .await
                .map_err(|error| error.to_string());
            let _sent = sender.send(UiMessage::RequestSaved { request, result });
            context.request_repaint();
        });
        self.set_notice(NoticeKind::Info, "Saving request…");
    }

    fn send_active(&mut self, context: &egui::Context) {
        if self.cancellation.is_some() {
            return;
        }
        let request = self.active().draft.clone();
        if request.url.trim().is_empty() {
            self.set_notice(
                NoticeKind::Error,
                "Enter an HTTP or HTTPS URL before sending",
            );
            return;
        }
        let workspace = self.snapshot.workspace.clone();
        let environment = self
            .selected_environment
            .and_then(|id| self.snapshot.environments.iter().find(|item| item.id == id))
            .cloned();
        let token = CancellationToken::new();
        self.cancellation = Some(token.clone());
        let service = self.service.clone();
        let sender = self.sender.clone();
        let context = context.clone();
        self.runtime.spawn(async move {
            let result = service
                .execute_request(&workspace, &request, environment.as_ref(), token)
                .await
                .map_err(|error| error.to_string());
            let _sent = sender.send(UiMessage::Executed {
                request_id: request.id,
                result,
            });
            context.request_repaint();
        });
        self.set_notice(NoticeKind::Info, "Sending request…");
    }

    fn handle_shortcuts(&mut self, context: &egui::Context) {
        let send = context
            .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        let save =
            context.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        let new_request =
            context.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::N));
        let focus_search =
            context.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::K));
        let cancel =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if send {
            self.send_active(context);
        }
        if save {
            self.save_active(context);
        }
        if new_request {
            self.new_request();
        }
        if focus_search {
            context.memory_mut(|memory| memory.request_focus(egui::Id::new("resource_search")));
        }
        if cancel {
            if let Some(token) = self.cancellation.take() {
                token.cancel();
                self.set_notice(NoticeKind::Warning, "Cancelling request…");
            } else {
                self.show_collection_dialog = false;
                self.show_environment_dialog = false;
                self.pending_close = None;
            }
        }
    }

    fn top_bar(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(self.palette.panel)
                    .stroke(Stroke::new(1.0, self.palette.border))
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(context, |ui| {
                ui.set_min_height(34.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Ferrum API")
                            .strong()
                            .color(self.palette.accent)
                            .size(19.0),
                    );
                    ui.label(
                        RichText::new("Local API workspace")
                            .color(self.palette.muted)
                            .size(12.0),
                    );
                    ui.separator();
                    ui.label(RichText::new(&self.snapshot.workspace.name).strong());
                    ui.add_space(12.0);
                    ui.label(RichText::new("Run with").color(self.palette.muted));
                    egui::ComboBox::from_id_salt("active_environment")
                        .width(150.0)
                        .selected_text(
                            self.selected_environment
                                .and_then(|id| {
                                    self.snapshot.environments.iter().find(|item| item.id == id)
                                })
                                .map_or("No environment", |item| item.name.as_str()),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.selected_environment,
                                None,
                                "No environment",
                            );
                            for environment in &self.snapshot.environments {
                                ui.selectable_value(
                                    &mut self.selected_environment,
                                    Some(environment.id),
                                    &environment.name,
                                );
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("New request")
                                        .strong()
                                        .color(Color32::from_rgb(30, 27, 24)),
                                )
                                .fill(self.palette.accent)
                                .min_size(egui::vec2(110.0, 32.0)),
                            )
                            .on_hover_text(shortcut_hint("Create a new request", "N"))
                            .clicked()
                        {
                            self.new_request();
                        }
                        let mut changed = false;
                        egui::ComboBox::from_id_salt("theme_mode")
                            .width(82.0)
                            .selected_text(self.theme_mode.label())
                            .show_ui(ui, |ui| {
                                changed |= ui
                                    .selectable_value(
                                        &mut self.theme_mode,
                                        ThemeMode::System,
                                        "System",
                                    )
                                    .changed();
                                changed |= ui
                                    .selectable_value(&mut self.theme_mode, ThemeMode::Dark, "Dark")
                                    .changed();
                                changed |= ui
                                    .selectable_value(
                                        &mut self.theme_mode,
                                        ThemeMode::Light,
                                        "Light",
                                    )
                                    .changed();
                            });
                        ui.label(RichText::new("Theme").color(self.palette.muted));
                        if changed {
                            self.apply_theme(context);
                        }
                    });
                });
            });
    }

    fn apply_theme(&mut self, context: &egui::Context) {
        let dark = match self.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => matches!(
                context.system_theme().unwrap_or_else(|| context.theme()),
                egui::Theme::Dark
            ),
        };
        self.palette = if dark {
            Palette::dark()
        } else {
            Palette::light()
        };
        configure_style(context, self.palette, dark);
    }

    fn navigation(&mut self, context: &egui::Context) {
        egui::SidePanel::left("navigation")
            .default_width(292.0)
            .min_width(250.0)
            .max_width(380.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(self.palette.panel)
                    .stroke(Stroke::new(1.0, self.palette.border))
                    .inner_margin(12.0),
            )
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    navigation_button(
                        ui,
                        &mut self.navigation,
                        Navigation::Collections,
                        self.snapshot.collections.len(),
                        self.palette,
                    );
                    navigation_button(
                        ui,
                        &mut self.navigation,
                        Navigation::Environments,
                        self.snapshot.environments.len(),
                        self.palette,
                    );
                    navigation_button(
                        ui,
                        &mut self.navigation,
                        Navigation::History,
                        self.snapshot.history.len(),
                        self.palette,
                    );
                });
                ui.add_space(12.0);
                match self.navigation {
                    Navigation::Collections => self.collections_panel(ui),
                    Navigation::Environments => self.environments_panel(ui),
                    Navigation::History => self.history_panel(ui),
                }
            });
    }

    fn resource_search(&mut self, ui: &mut egui::Ui, hint: &str) {
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .id(egui::Id::new("resource_search"))
                .hint_text(format!("{hint}  {}", shortcut_label("K")))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 8)),
        );
    }

    fn collections_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("COLLECTIONS")
                    .strong()
                    .color(self.palette.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("New collection").clicked() {
                    self.show_collection_dialog = true;
                }
            });
        });
        ui.add_space(8.0);
        self.resource_search(ui, "Search collections and requests");
        ui.add_space(10.0);
        let needle = self.search.to_ascii_lowercase();
        let collections = self.snapshot.collections.clone();
        let requests = self.snapshot.requests.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if collections.is_empty() && requests.is_empty() {
                empty_sidebar(
                    ui,
                    "No saved requests yet",
                    "Create a collection to organize requests, or save the open request without one.",
                    self.palette,
                );
                if ui.button("Create first collection").clicked() {
                    self.show_collection_dialog = true;
                }
                return;
            }
            for collection in collections {
                let collection_matches = collection.name.to_ascii_lowercase().contains(&needle);
                let matching_requests = requests
                    .iter()
                    .filter(|request| {
                        request.collection_id == Some(collection.id)
                            && (needle.is_empty()
                                || request.name.to_ascii_lowercase().contains(&needle)
                                || request.url.to_ascii_lowercase().contains(&needle))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if collection_matches || !matching_requests.is_empty() {
                    egui::CollapsingHeader::new(
                        RichText::new(format!(
                            "{}  ({})",
                            collection.name,
                            matching_requests.len()
                        ))
                        .strong(),
                    )
                    .default_open(true)
                    .show(ui, |ui| {
                        for request in matching_requests {
                            if request_link(ui, &request, self.palette).clicked() {
                                self.open_request(request);
                            }
                        }
                    });
                }
            }
            let ungrouped = requests
                .into_iter()
                .filter(|request| request.collection_id.is_none())
                .collect::<Vec<_>>();
            if !ungrouped.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("UNGROUPED").small().color(self.palette.muted));
                for request in ungrouped {
                    let matches = needle.is_empty()
                        || request.name.to_ascii_lowercase().contains(&needle)
                        || request.url.to_ascii_lowercase().contains(&needle);
                    if matches && request_link(ui, &request, self.palette).clicked() {
                        self.open_request(request);
                    }
                }
            }
        });
    }

    fn environments_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("ENVIRONMENTS")
                    .strong()
                    .color(self.palette.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("New environment").clicked() {
                    self.show_environment_dialog = true;
                }
            });
        });
        ui.add_space(8.0);
        self.resource_search(ui, "Search environments");
        ui.add_space(10.0);
        let needle = self.search.to_ascii_lowercase();
        let environments = self.snapshot.environments.clone();
        if environments.is_empty() {
            empty_sidebar(
                ui,
                "No environments",
                "Environments keep base URLs and credentials separate from saved requests.",
                self.palette,
            );
            if ui.button("Create first environment").clicked() {
                self.show_environment_dialog = true;
            }
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for environment in environments.into_iter().filter(|item| {
                needle.is_empty() || item.name.to_ascii_lowercase().contains(&needle)
            }) {
                let selected = self.selected_environment == Some(environment.id);
                let label = format!(
                    "{}\n{} variables",
                    environment.name,
                    environment.variables.len()
                );
                if ui
                    .add_sized(
                        [ui.available_width(), 48.0],
                        egui::Button::new(label).selected(selected),
                    )
                    .clicked()
                {
                    self.selected_environment = Some(environment.id);
                }
            }
        });
    }

    fn history_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("REQUEST HISTORY")
                    .strong()
                    .color(self.palette.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("history_filter")
                    .width(102.0)
                    .selected_text(self.history_filter.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.history_filter,
                            HistoryFilter::All,
                            "All results",
                        );
                        ui.selectable_value(
                            &mut self.history_filter,
                            HistoryFilter::Success,
                            "Successful",
                        );
                        ui.selectable_value(
                            &mut self.history_filter,
                            HistoryFilter::Failed,
                            "Failed",
                        );
                    });
            });
        });
        ui.add_space(8.0);
        self.resource_search(ui, "Search request history");
        ui.add_space(10.0);
        let needle = self.search.to_ascii_lowercase();
        let history = self.snapshot.history.clone();
        let filtered = history
            .into_iter()
            .filter(|entry| {
                let search_matches = needle.is_empty()
                    || entry.url.to_ascii_lowercase().contains(&needle)
                    || entry.method.as_str().to_ascii_lowercase().contains(&needle);
                let filter_matches = match self.history_filter {
                    HistoryFilter::All => true,
                    HistoryFilter::Success => entry.status.is_some_and(|status| status < 400),
                    HistoryFilter::Failed => entry.status.is_none_or(|status| status >= 400),
                };
                search_matches && filter_matches
            })
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            empty_sidebar(
                ui,
                "No matching history",
                "Sent requests appear here with status, duration, and time.",
                self.palette,
            );
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in filtered {
                let status = history_status(&entry, self.palette);
                let frame = egui::Frame::new()
                    .fill(self.palette.canvas)
                    .stroke(Stroke::new(1.0, self.palette.border))
                    .inner_margin(10.0);
                let response = frame
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(entry.method.as_str())
                                    .strong()
                                    .color(method_color(&entry.method, self.palette)),
                            );
                            ui.label(RichText::new(status.0).strong().color(status.1));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(
                                            entry.created_at.format("%b %d · %H:%M").to_string(),
                                        )
                                        .color(self.palette.muted),
                                    );
                                },
                            );
                        });
                        ui.add(
                            egui::Label::new(RichText::new(&entry.url).monospace())
                                .truncate()
                                .selectable(false),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} ms · Open request",
                                entry.duration_ms.unwrap_or_default()
                            ))
                            .small()
                            .color(self.palette.muted),
                        );
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.clicked() {
                    self.reopen_history(&entry);
                }
                ui.add_space(7.0);
            }
        });
    }

    fn reopen_history(&mut self, entry: &HistoryEntry) {
        if let Some(request) = entry
            .request_id
            .and_then(|id| {
                self.snapshot
                    .requests
                    .iter()
                    .find(|request| request.id == id)
            })
            .cloned()
        {
            self.open_request(request);
            return;
        }
        let mut request = SavedRequest::blank(self.snapshot.workspace.id);
        request.method = entry.method.clone();
        request.url.clone_from(&entry.url);
        request.headers.clone_from(&entry.request_headers);
        self.open_request(request);
    }

    fn request_workspace(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::bottom("response_panel")
            .resizable(true)
            .default_height(300.0)
            .min_height(170.0)
            .max_height(560.0)
            .frame(
                egui::Frame::new()
                    .fill(self.palette.canvas)
                    .stroke(Stroke::new(1.0, self.palette.border))
                    .inner_margin(14.0),
            )
            .show(context, |ui| self.response_view(ui, context));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(self.palette.canvas)
                    .inner_margin(14.0),
            )
            .show(context, |ui| {
                self.tab_strip(ui);
                ui.add_space(8.0);
                self.notice_banner(ui);
                self.execution_toolbar(ui, context);
                ui.add_space(8.0);
                self.request_metadata(ui, context);
                ui.add_space(14.0);
                self.request_editor_tabs(ui);
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("request_editor")
                    .show(ui, |ui| self.request_editor_body(ui));
            });
    }

    fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("request_tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        let active = index == self.active_tab;
                        let mut title = truncate_title(&tab.draft.name, 26);
                        if tab.dirty {
                            title.push_str(" (unsaved)");
                        }
                        let frame = egui::Frame::new()
                            .fill(if active {
                                self.palette.elevated
                            } else {
                                self.palette.panel
                            })
                            .stroke(Stroke::new(
                                if active { 2.0 } else { 1.0 },
                                if active {
                                    self.palette.accent
                                } else {
                                    self.palette.border
                                },
                            ))
                            .inner_margin(egui::Margin::symmetric(10, 6));
                        frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new(title).color(if active {
                                            self.palette.text
                                        } else {
                                            self.palette.muted
                                        }))
                                        .frame(false)
                                        .min_size(egui::vec2(120.0, 24.0)),
                                    )
                                    .clicked()
                                {
                                    selected = Some(index);
                                }
                                if active
                                    && ui
                                        .small_button("Close")
                                        .on_hover_text(if tab.dirty {
                                            "Close request; confirmation required"
                                        } else {
                                            "Close request"
                                        })
                                        .clicked()
                                {
                                    close = Some(index);
                                }
                            });
                        });
                    }
                    if ui
                        .add_sized([82.0, 36.0], egui::Button::new("New tab"))
                        .on_hover_text(shortcut_hint("Create a new request tab", "N"))
                        .clicked()
                    {
                        selected = Some(self.tabs.len());
                    }
                });
            });
        if let Some(index) = selected {
            if index == self.tabs.len() {
                self.new_request();
            } else {
                self.active_tab = index;
            }
        }
        if let Some(index) = close {
            self.request_close(index);
        }
    }

    fn execution_toolbar(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let running = self.cancellation.is_some();
        let palette = self.palette;
        ui.horizontal(|ui| {
            let active = &mut self.tabs[self.active_tab];
            let mut changed = false;
            egui::ComboBox::from_id_salt("method")
                .width(96.0)
                .selected_text(
                    RichText::new(active.draft.method.as_str())
                        .strong()
                        .color(method_color(&active.draft.method, palette)),
                )
                .show_ui(ui, |ui| {
                    for method in [
                        HttpMethod::Get,
                        HttpMethod::Post,
                        HttpMethod::Put,
                        HttpMethod::Patch,
                        HttpMethod::Delete,
                        HttpMethod::Head,
                        HttpMethod::Options,
                    ] {
                        changed |= ui
                            .selectable_value(
                                &mut active.draft.method,
                                method.clone(),
                                method.as_str(),
                            )
                            .changed();
                    }
                });
            let action_width = 106.0;
            let url_width = (ui.available_width() - action_width - 8.0).max(180.0);
            changed |= ui
                .add_sized(
                    [url_width, 34.0],
                    egui::TextEdit::singleline(&mut active.draft.url)
                        .hint_text(
                            RichText::new("https://api.example.com/{{resource}}")
                                .color(palette.muted),
                        )
                        .font(FontId::monospace(14.0))
                        .margin(egui::Margin::symmetric(10, 8)),
                )
                .changed();
            active.dirty |= changed;
            if running {
                if ui
                    .add_sized(
                        [action_width, 34.0],
                        egui::Button::new(
                            RichText::new("Cancel request")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(palette.danger),
                    )
                    .on_hover_text("Cancel the active network operation (Escape)")
                    .clicked()
                {
                    if let Some(token) = self.cancellation.take() {
                        token.cancel();
                    }
                }
            } else {
                let can_send = !active.draft.url.trim().is_empty();
                if ui
                    .add_enabled(
                        can_send,
                        egui::Button::new(
                            RichText::new("Send request")
                                .strong()
                                .color(Color32::from_rgb(30, 27, 24)),
                        )
                        .fill(palette.accent)
                        .min_size(egui::vec2(action_width, 34.0)),
                    )
                    .on_hover_text(if can_send {
                        shortcut_hint("Send request", "Enter")
                    } else {
                        "Enter a URL to enable Send".into()
                    })
                    .clicked()
                {
                    self.send_active(context);
                }
            }
        });
        if self.active().draft.url.trim().is_empty() {
            ui.label(
                RichText::new("Enter an HTTP or HTTPS URL to send this request.")
                    .small()
                    .color(self.palette.warning),
            );
        }
    }

    fn request_metadata(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let collections = self.snapshot.collections.clone();
        let palette = self.palette;
        let mut save_clicked = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Request name").color(palette.muted));
            let active = &mut self.tabs[self.active_tab];
            active.dirty |= ui
                .add_sized(
                    [220.0, 30.0],
                    egui::TextEdit::singleline(&mut active.draft.name)
                        .hint_text("Name this request"),
                )
                .changed();
            ui.label(RichText::new("Save to").color(palette.muted));
            let selected_name = active
                .draft
                .collection_id
                .and_then(|id| collections.iter().find(|item| item.id == id))
                .map_or("Ungrouped", |item| item.name.as_str());
            egui::ComboBox::from_id_salt("request_collection")
                .width(190.0)
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    active.dirty |= ui
                        .selectable_value(&mut active.draft.collection_id, None, "Ungrouped")
                        .changed();
                    for collection in &collections {
                        active.dirty |= ui
                            .selectable_value(
                                &mut active.draft.collection_id,
                                Some(collection.id),
                                &collection.name,
                            )
                            .changed();
                    }
                });
            if active.dirty {
                ui.label(RichText::new("Unsaved changes").color(palette.warning));
            } else {
                ui.label(RichText::new("Saved locally").color(palette.success));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                save_clicked = ui
                    .add_enabled(
                        active.dirty,
                        egui::Button::new("Save changes").min_size(egui::vec2(110.0, 30.0)),
                    )
                    .on_hover_text(shortcut_hint("Save request", "S"))
                    .clicked();
            });
        });
        if save_clicked {
            self.save_active(context);
        }
    }

    fn request_editor_tabs(&mut self, ui: &mut egui::Ui) {
        let request = &self.active().draft;
        let params = request
            .query
            .iter()
            .filter(|row| row.enabled && !row.key.is_empty())
            .count();
        let headers = request
            .headers
            .iter()
            .filter(|row| row.enabled && !row.key.is_empty())
            .count();
        ui.horizontal(|ui| {
            section_tab(
                ui,
                &mut self.editor_tab,
                RequestEditorTab::Params,
                &format!("Params  {params}"),
                self.palette,
            );
            section_tab(
                ui,
                &mut self.editor_tab,
                RequestEditorTab::Headers,
                &format!("Headers  {headers}"),
                self.palette,
            );
            section_tab(
                ui,
                &mut self.editor_tab,
                RequestEditorTab::Body,
                "Body",
                self.palette,
            );
        });
    }

    fn request_editor_body(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let editor_tab = self.editor_tab;
        let active = self.active_mut();
        let changed = match editor_tab {
            RequestEditorTab::Params => key_value_rows(
                ui,
                &mut active.draft.query,
                "Parameter",
                "Add parameter",
                palette,
            ),
            RequestEditorTab::Headers => key_value_rows(
                ui,
                &mut active.draft.headers,
                "Header",
                "Add header",
                palette,
            ),
            RequestEditorTab::Body => body_editor(ui, &mut active.draft.body, palette),
        };
        active.dirty |= changed;
    }

    fn response_view(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let response = self.active().response.clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Response").strong().size(17.0));
            ui.label(
                RichText::new("Drag the divider to resize")
                    .small()
                    .color(self.palette.muted),
            );
            if let Some(response) = &response {
                let status_color = status_color(response.status, self.palette);
                ui.label(
                    RichText::new(format!("{} {}", response.status, response.status_text))
                        .strong()
                        .color(status_color)
                        .background_color(self.palette.elevated),
                );
                ui.label(
                    RichText::new(format!(
                        "{:.0} ms",
                        response.duration.as_secs_f64() * 1000.0
                    ))
                    .color(self.palette.muted),
                );
                ui.label(RichText::new(format_bytes(response.body.size)).color(self.palette.muted));
                if response.body.truncated {
                    ui.label(
                        RichText::new("Preview capped; complete body saved on disk")
                            .strong()
                            .color(self.palette.warning),
                    );
                }
            }
        });
        ui.add_space(4.0);
        let Some(response) = response else {
            ui.separator();
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(
                    RichText::new("No response yet")
                        .strong()
                        .size(18.0)
                        .color(self.palette.text),
                );
                ui.label(
                    RichText::new(
                        "Enter a URL and send the request. Status, headers, timing, and body will appear here.",
                    )
                    .color(self.palette.muted),
                );
                ui.add_space(10.0);
                let can_send = !self.active().draft.url.trim().is_empty();
                if ui
                    .add_enabled(can_send, egui::Button::new("Send current request"))
                    .clicked()
                {
                    self.send_active(context);
                }
            });
            return;
        };
        ui.horizontal(|ui| {
            section_tab(
                ui,
                &mut self.response_tab,
                ResponseTab::Pretty,
                "Pretty",
                self.palette,
            );
            section_tab(
                ui,
                &mut self.response_tab,
                ResponseTab::Raw,
                "Raw",
                self.palette,
            );
            section_tab(
                ui,
                &mut self.response_tab,
                ResponseTab::Headers,
                &format!("Headers  {}", response.headers.len()),
                self.palette,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let body_text = response_body_text(&response, self.response_tab);
                if ui.button("Copy response").clicked() {
                    ui.ctx().copy_text(body_text.clone());
                    self.set_notice(NoticeKind::Success, "Response copied to clipboard");
                }
                if self.response_tab != ResponseTab::Headers {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.response_search)
                            .hint_text("Find in response")
                            .desired_width(180.0),
                    );
                    if !self.response_search.is_empty() {
                        let matches = body_text
                            .to_ascii_lowercase()
                            .matches(&self.response_search.to_ascii_lowercase())
                            .count();
                        ui.label(
                            RichText::new(format!("{matches} matches"))
                                .small()
                                .color(self.palette.muted),
                        );
                    }
                }
            });
        });
        ui.separator();
        match self.response_tab {
            ResponseTab::Headers => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("response_headers")
                        .num_columns(2)
                        .striped(true)
                        .spacing([18.0, 10.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Header").strong().color(self.palette.muted));
                            ui.label(RichText::new("Value").strong().color(self.palette.muted));
                            ui.end_row();
                            for header in &response.headers {
                                ui.label(RichText::new(&header.key).monospace().strong());
                                ui.label(RichText::new(&header.value).monospace());
                                ui.end_row();
                            }
                        });
                });
            }
            ResponseTab::Pretty | ResponseTab::Raw => {
                let body = response_body_text(&response, self.response_tab);
                egui::ScrollArea::both()
                    .id_salt("response_body")
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(body)
                                    .monospace()
                                    .size(13.5)
                                    .color(self.palette.text),
                            )
                            .selectable(true)
                            .wrap(),
                        );
                    });
            }
        }
    }

    fn environment_workspace(&mut self, context: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.palette.canvas).inner_margin(20.0))
            .show(context, |ui| {
                self.notice_banner(ui);
                let selected_index = self.selected_environment.and_then(|id| {
                    self.snapshot.environments.iter().position(|item| item.id == id)
                });
                let Some(index) = selected_index else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(90.0);
                        ui.label(
                            RichText::new("Choose an environment")
                                .strong()
                                .size(22.0)
                                .color(self.palette.text),
                        );
                        ui.label(
                            RichText::new(
                                "Select an environment from the sidebar or create one to manage variables.",
                            )
                            .color(self.palette.muted),
                        );
                        ui.add_space(12.0);
                        if ui.button("Create environment").clicked() {
                            self.show_environment_dialog = true;
                        }
                    });
                    return;
                };
                let mut save = false;
                let environment = &mut self.snapshot.environments[index];
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&environment.name)
                                .strong()
                                .size(22.0)
                                .color(self.palette.text),
                        );
                        ui.label(
                            RichText::new(
                                "Current values stay on this device. Sensitive values are stored in the operating-system credential vault.",
                            )
                            .color(self.palette.muted),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        save = ui
                            .add_enabled(
                                self.environment_dirty,
                                egui::Button::new("Save environment")
                                    .min_size(egui::vec2(130.0, 34.0)),
                            )
                            .clicked();
                        if self.environment_dirty {
                            ui.label(
                                RichText::new("Unsaved changes").color(self.palette.warning),
                            );
                        } else {
                            ui.label(RichText::new("Saved securely").color(self.palette.success));
                        }
                    });
                });
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Variables").strong().size(17.0));
                    ui.label(
                        RichText::new(format!("{} total", environment.variables.len()))
                            .color(self.palette.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Add variable").clicked() {
                            environment.variables.push(blank_variable());
                            self.environment_dirty = true;
                        }
                    });
                });
                ui.separator();
                self.environment_dirty |=
                    environment_variable_rows(ui, &mut environment.variables, self.palette);
                let environment = environment.clone();
                if save {
                    self.save_environment(environment, context);
                }
            });
    }

    fn save_environment(&mut self, environment: Environment, context: &egui::Context) {
        let service = self.service.clone();
        let sender = self.sender.clone();
        let context = context.clone();
        self.runtime.spawn(async move {
            let result = service
                .save_environment(&environment)
                .await
                .map_err(|error| error.to_string());
            let _sent = sender.send(UiMessage::EnvironmentSaved {
                environment,
                result,
            });
            context.request_repaint();
        });
        self.set_notice(NoticeKind::Info, "Saving environment…");
    }

    fn notice_banner(&self, ui: &mut egui::Ui) {
        if self.notice_kind == NoticeKind::Info && self.status_message.starts_with("Ready") {
            return;
        }
        let (label, color) = match self.notice_kind {
            NoticeKind::Info => ("In progress", self.palette.muted),
            NoticeKind::Success => ("Success", self.palette.success),
            NoticeKind::Warning => ("Attention", self.palette.warning),
            NoticeKind::Error => ("Error", self.palette.danger),
        };
        egui::Frame::new()
            .fill(self.palette.elevated)
            .stroke(Stroke::new(1.0, color))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).strong().color(color));
                    ui.label(&self.status_message);
                });
            });
        ui.add_space(8.0);
    }

    fn dialogs(&mut self, context: &egui::Context) {
        self.collection_dialog(context);
        self.environment_dialog(context);
        self.close_confirmation(context);
    }

    fn collection_dialog(&mut self, context: &egui::Context) {
        if !self.show_collection_dialog {
            return;
        }
        let mut open = true;
        let mut create = false;
        egui::Window::new("Create collection")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.set_width(380.0);
                ui.label(
                    "Collections group related requests and can be searched from the sidebar.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Collection name").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.collection_name)
                        .hint_text("For example: Customer API")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    create = ui
                        .add_enabled(
                            !self.collection_name.trim().is_empty(),
                            egui::Button::new("Create collection"),
                        )
                        .clicked();
                    if ui.button("Cancel").clicked() {
                        self.show_collection_dialog = false;
                    }
                });
            });
        self.show_collection_dialog &= open;
        if create {
            self.create_collection(context);
            self.show_collection_dialog = false;
        }
    }

    fn create_collection(&mut self, context: &egui::Context) {
        let service = self.service.clone();
        let workspace = self.snapshot.workspace.clone();
        let name = self.collection_name.trim().to_owned();
        let sender = self.sender.clone();
        let context = context.clone();
        self.runtime.spawn(async move {
            let result = service
                .create_collection(&workspace, &name)
                .await
                .map_err(|error| error.to_string());
            let _sent = sender.send(UiMessage::CollectionCreated(result));
            context.request_repaint();
        });
        self.set_notice(NoticeKind::Info, "Creating collection…");
    }

    fn environment_dialog(&mut self, context: &egui::Context) {
        if !self.show_environment_dialog {
            return;
        }
        let mut open = true;
        let mut create = false;
        egui::Window::new("Create environment")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.set_width(400.0);
                ui.label(
                    "Use environments for base URLs, tokens, and values that change between deployments.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Environment name").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.environment_name)
                        .hint_text("For example: Development")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    create = ui
                        .add_enabled(
                            !self.environment_name.trim().is_empty(),
                            egui::Button::new("Create environment"),
                        )
                        .clicked();
                    if ui.button("Cancel").clicked() {
                        self.show_environment_dialog = false;
                    }
                });
            });
        self.show_environment_dialog &= open;
        if create {
            let environment = Environment {
                id: EnvironmentId::new(),
                workspace_id: self.snapshot.workspace.id,
                name: self.environment_name.trim().to_owned(),
                variables: vec![blank_variable()],
            };
            self.save_environment(environment, context);
            self.show_environment_dialog = false;
        }
    }

    fn close_confirmation(&mut self, context: &egui::Context) {
        let Some(index) = self.pending_close else {
            return;
        };
        let title = self.tabs[index].draft.name.clone();
        egui::Window::new("Unsaved request")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.set_width(400.0);
                ui.label(format!(
                    "“{title}” has unsaved changes. Closing it now will discard those edits."
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Discard changes").color(Color32::WHITE),
                            )
                            .fill(self.palette.danger),
                        )
                        .clicked()
                    {
                        self.close_tab(index);
                        self.pending_close = None;
                    }
                    if ui.button("Keep editing").clicked() {
                        self.pending_close = None;
                    }
                });
            });
    }

    fn status_bar(&self, context: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(self.palette.panel)
                    .stroke(Stroke::new(1.0, self.palette.border))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    let label = match self.notice_kind {
                        NoticeKind::Info => "Status",
                        NoticeKind::Success => "Success",
                        NoticeKind::Warning => "Attention",
                        NoticeKind::Error => "Error",
                    };
                    ui.label(
                        RichText::new(label)
                            .strong()
                            .color(notice_color(self.notice_kind, self.palette)),
                    );
                    ui.label(RichText::new(&self.status_message).color(self.palette.muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("TLS verification enabled · Data stays local")
                                .color(self.palette.success),
                        );
                    });
                });
            });
    }

    fn capture_audit_frame(&mut self, context: &egui::Context) {
        let Some(path) = self.audit_screenshot.clone() else {
            return;
        };
        let screenshot = context.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(screenshot) = screenshot {
            let mut rgba = Vec::with_capacity(screenshot.pixels.len() * 4);
            for pixel in &screenshot.pixels {
                rgba.extend_from_slice(&pixel.to_array());
            }
            let [width, height] = screenshot.size;
            if image::save_buffer(
                &path,
                &rgba,
                u32::try_from(width).unwrap_or(u32::MAX),
                u32::try_from(height).unwrap_or(u32::MAX),
                image::ColorType::Rgba8,
            )
            .is_ok()
            {
                self.audit_screenshot = None;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        self.rendered_frames = self.rendered_frames.saturating_add(1);
        if self.rendered_frames >= 3 && !self.screenshot_requested {
            self.screenshot_requested = true;
            context.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        context.request_repaint_after(Duration::from_millis(40));
    }
}

impl eframe::App for FerrumApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.capture_audit_frame(context);
        self.process_messages();
        self.handle_shortcuts(context);
        self.top_bar(context);
        self.status_bar(context);
        self.navigation(context);
        match self.navigation {
            Navigation::Environments => self.environment_workspace(context),
            Navigation::Collections | Navigation::History => self.request_workspace(context),
        }
        self.dialogs(context);
        if self.cancellation.is_some() {
            context.request_repaint_after(Duration::from_millis(80));
        }
    }
}

fn configure_style(context: &egui::Context, palette: Palette, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.panel;
    visuals.extreme_bg_color = palette.canvas;
    visuals.override_text_color = Some(palette.text);
    visuals.widgets.inactive.bg_fill = palette.elevated;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.hovered.bg_fill = palette.accent_soft;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, palette.accent);
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, palette.accent);
    visuals.widgets.open.bg_stroke = Stroke::new(2.0, palette.accent);
    visuals.selection.bg_fill = palette.accent_soft;
    visuals.selection.stroke = Stroke::new(2.0, palette.accent);
    context.set_visuals(visuals);
    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.spacing.button_padding = egui::vec2(11.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(13.5));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(21.0));
    context.set_style(style);
}

fn navigation_button(
    ui: &mut egui::Ui,
    current: &mut Navigation,
    value: Navigation,
    count: usize,
    palette: Palette,
) {
    let selected = *current == value;
    let label = format!("{}\n{count}", value.label());
    let button = egui::Button::new(RichText::new(label).strong().color(if selected {
        palette.text
    } else {
        palette.muted
    }))
    .selected(selected)
    .min_size(egui::vec2(78.0, 34.0));
    if ui.add_sized([88.0, 46.0], button).clicked() {
        *current = value;
    }
}

fn section_tab<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
    palette: Palette,
) {
    let selected = *current == value;
    let response = ui.add(
        egui::Button::new(RichText::new(label).strong().color(if selected {
            palette.accent
        } else {
            palette.muted
        }))
        .frame(false)
        .min_size(egui::vec2(76.0, 34.0)),
    );
    if selected {
        ui.painter().line_segment(
            [response.rect.left_bottom(), response.rect.right_bottom()],
            Stroke::new(2.0, palette.accent),
        );
    }
    if response.clicked() {
        *current = value;
    }
}

fn request_link(ui: &mut egui::Ui, request: &SavedRequest, palette: Palette) -> egui::Response {
    let response = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(8, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(request.method.as_str())
                        .strong()
                        .color(method_color(&request.method, palette)),
                );
                ui.label(RichText::new(&request.name).color(palette.text));
            });
            if !request.url.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(&request.url)
                            .monospace()
                            .small()
                            .color(palette.muted),
                    )
                    .truncate(),
                );
            }
        })
        .response
        .interact(egui::Sense::click());
    response.on_hover_text(format!("Open {}\n{}", request.name, request.url))
}

fn empty_sidebar(ui: &mut egui::Ui, title: &str, body: &str, palette: Palette) {
    ui.add_space(28.0);
    ui.label(RichText::new(title).strong().size(16.0).color(palette.text));
    ui.label(RichText::new(body).color(palette.muted));
    ui.add_space(8.0);
}

fn key_value_rows(
    ui: &mut egui::Ui,
    rows: &mut Vec<KeyValue>,
    key_label: &str,
    add_label: &str,
    palette: Palette,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    ui.horizontal(|ui| {
        ui.add_sized(
            [78.0, 24.0],
            egui::Label::new(RichText::new("Include").strong().color(palette.muted)),
        );
        let width = ui.available_width();
        ui.add_sized(
            [width * 0.34, 24.0],
            egui::Label::new(RichText::new(key_label).strong().color(palette.muted)),
        );
        ui.add_sized(
            [(width * 0.56).max(160.0), 24.0],
            egui::Label::new(RichText::new("Value").strong().color(palette.muted)),
        );
        ui.label(RichText::new("Action").strong().color(palette.muted));
    });
    for (index, row) in rows.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui
                .add_sized([78.0, 30.0], egui::Checkbox::new(&mut row.enabled, "Use"))
                .on_hover_text("Include this row when sending")
                .changed();
            let width = ui.available_width();
            changed |= ui
                .add_sized(
                    [width * 0.34, 30.0],
                    egui::TextEdit::singleline(&mut row.key).hint_text(key_label),
                )
                .changed();
            changed |= ui
                .add_sized(
                    [(width * 0.56).max(160.0), 30.0],
                    egui::TextEdit::singleline(&mut row.value).hint_text("Value"),
                )
                .changed();
            if ui.button("Remove").clicked() {
                remove = Some(index);
            }
        });
        ui.add_space(3.0);
    }
    if let Some(index) = remove {
        rows.remove(index);
        changed = true;
    }
    ui.add_space(4.0);
    if ui.button(add_label).clicked() {
        rows.push(KeyValue::default());
        changed = true;
    }
    changed
}

fn environment_variable_rows(
    ui: &mut egui::Ui,
    variables: &mut Vec<Variable>,
    palette: Palette,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    ui.horizontal(|ui| {
        ui.add_sized(
            [78.0, 24.0],
            egui::Label::new(RichText::new("Include").strong().color(palette.muted)),
        );
        let width = ui.available_width();
        ui.add_sized(
            [width * 0.27, 24.0],
            egui::Label::new(RichText::new("Variable name").strong().color(palette.muted)),
        );
        ui.add_sized(
            [width * 0.43, 24.0],
            egui::Label::new(RichText::new("Current value").strong().color(palette.muted)),
        );
        ui.add_sized(
            [120.0, 24.0],
            egui::Label::new(RichText::new("Storage").strong().color(palette.muted)),
        );
        ui.label(RichText::new("Action").strong().color(palette.muted));
    });
    for (index, variable) in variables.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui
                .add_sized(
                    [78.0, 30.0],
                    egui::Checkbox::new(&mut variable.enabled, "Use"),
                )
                .changed();
            let width = ui.available_width();
            changed |= ui
                .add_sized(
                    [width * 0.27, 30.0],
                    egui::TextEdit::singleline(&mut variable.name).hint_text("base_url"),
                )
                .changed();
            let value_editor = egui::TextEdit::singleline(&mut variable.current_value)
                .password(variable.sensitive)
                .hint_text(if variable.sensitive {
                    "Stored securely — enter to replace"
                } else {
                    "Local current value"
                });
            changed |= ui
                .add_sized([(width * 0.43).max(220.0), 30.0], value_editor)
                .changed();
            changed |= ui
                .add_sized(
                    [120.0, 30.0],
                    egui::Checkbox::new(&mut variable.sensitive, "Sensitive"),
                )
                .on_hover_text("Sensitive values are stored in the OS credential vault")
                .changed();
            if ui.button("Remove").clicked() {
                remove = Some(index);
            }
        });
        if variable.sensitive {
            ui.horizontal(|ui| {
                ui.add_space(86.0);
                ui.label(
                    RichText::new("Secure value: operating-system credential vault")
                        .small()
                        .color(palette.success),
                );
            });
        }
        ui.add_space(4.0);
    }
    if let Some(index) = remove {
        variables.remove(index);
        changed = true;
    }
    changed
}

fn blank_variable() -> Variable {
    Variable {
        name: String::new(),
        current_value: String::new(),
        initial_value: None,
        sensitive: false,
        enabled: true,
    }
}

fn body_editor(ui: &mut egui::Ui, body: &mut RequestBody, palette: Palette) -> bool {
    let current = match body {
        RequestBody::None => 0,
        RequestBody::Json(_) => 1,
        RequestBody::Text { .. } => 2,
    };
    let mut selected = current;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Body type").strong().color(palette.muted));
        egui::ComboBox::from_id_salt("body_kind")
            .width(150.0)
            .selected_text(["No body", "JSON", "Text"][selected])
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, 0, "No body");
                ui.selectable_value(&mut selected, 1, "JSON");
                ui.selectable_value(&mut selected, 2, "Text");
            });
    });
    ui.add_space(8.0);
    let mut changed = selected != current;
    if selected != current {
        *body = match selected {
            1 => RequestBody::Json("{\n  \n}".into()),
            2 => RequestBody::Text {
                content_type: "text/plain".into(),
                content: String::new(),
            },
            _ => RequestBody::None,
        };
    }
    match body {
        RequestBody::None => {
            empty_sidebar(
                ui,
                "This request has no body",
                "Choose JSON or Text when the endpoint expects request content.",
                palette,
            );
        }
        RequestBody::Json(content) => {
            changed |= ui
                .add(
                    egui::TextEdit::multiline(content)
                        .code_editor()
                        .desired_rows(12)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            ui.horizontal(|ui| {
                if ui.button("Format JSON").clicked() {
                    match serde_json::from_str::<serde_json::Value>(content) {
                        Ok(value) => {
                            if let Ok(formatted) = serde_json::to_string_pretty(&value) {
                                *content = formatted;
                                changed = true;
                            }
                        }
                        Err(error) => {
                            ui.label(
                                RichText::new(format!("Invalid JSON: {error}"))
                                    .color(palette.danger),
                            );
                        }
                    }
                }
                ui.label(RichText::new("Content-Type: application/json").color(palette.muted));
            });
        }
        RequestBody::Text {
            content_type,
            content,
        } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Content-Type").strong());
                changed |= ui
                    .add_sized(
                        [260.0, 30.0],
                        egui::TextEdit::singleline(content_type).hint_text("text/plain"),
                    )
                    .changed();
            });
            ui.add_space(8.0);
            changed |= ui
                .add(
                    egui::TextEdit::multiline(content)
                        .code_editor()
                        .desired_rows(12)
                        .desired_width(f32::INFINITY),
                )
                .changed();
        }
    }
    changed
}

fn upsert_request(requests: &mut Vec<SavedRequest>, request: SavedRequest) {
    if let Some(existing) = requests.iter_mut().find(|item| item.id == request.id) {
        *existing = request;
    } else {
        requests.insert(0, request);
    }
}

fn truncate_title(value: &str, max_chars: usize) -> String {
    let mut characters = value.chars();
    let title = characters.by_ref().take(max_chars).collect::<String>();
    if characters.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

fn response_body_text(response: &HttpResponse, mode: ResponseTab) -> String {
    let raw = String::from_utf8_lossy(&response.body.preview);
    if mode == ResponseTab::Pretty {
        serde_json::from_str::<serde_json::Value>(&raw)
            .and_then(|json| serde_json::to_string_pretty(&json))
            .unwrap_or_else(|_| raw.into_owned())
    } else {
        raw.into_owned()
    }
}

fn method_color(method: &HttpMethod, palette: Palette) -> Color32 {
    match method {
        HttpMethod::Get | HttpMethod::Head => palette.success,
        HttpMethod::Post => palette.accent,
        HttpMethod::Put | HttpMethod::Patch => palette.warning,
        HttpMethod::Delete => palette.danger,
        HttpMethod::Options | HttpMethod::Custom(_) => Color32::from_rgb(143, 159, 255),
    }
}

fn status_color(status: u16, palette: Palette) -> Color32 {
    match status {
        200..=399 => palette.success,
        400..=499 => palette.warning,
        _ => palette.danger,
    }
}

fn history_status(entry: &HistoryEntry, palette: Palette) -> (String, Color32) {
    match entry.status {
        Some(status @ 200..=399) => (format!("{status} Success"), palette.success),
        Some(status @ 400..=499) => (format!("{status} Client error"), palette.warning),
        Some(status) => (format!("{status} Failed"), palette.danger),
        None => ("Network error".into(), palette.danger),
    }
}

fn notice_color(kind: NoticeKind, palette: Palette) -> Color32 {
    match kind {
        NoticeKind::Info => palette.muted,
        NoticeKind::Success => palette.success,
        NoticeKind::Warning => palette.warning,
        NoticeKind::Error => palette.danger,
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{bytes} B")
    } else if bytes < 1_048_576 {
        let tenths = bytes.saturating_mul(10) / 1_024;
        format!("{}.{:01} KiB", tenths / 10, tenths % 10)
    } else {
        let tenths = bytes.saturating_mul(10) / 1_048_576;
        format!("{}.{:01} MiB", tenths / 10, tenths % 10)
    }
}

fn shortcut_label(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{key}")
    } else {
        format!("Ctrl+{key}")
    }
}

fn shortcut_hint(action: &str, key: &str) -> String {
    format!("{action} ({})", shortcut_label(key))
}

fn sample_response() -> HttpResponse {
    let body = br#"{"data":{"users":[{"id":1,"name":"Ada Lovelace","role":"Engineer"},{"id":2,"name":"Grace Hopper","role":"Admiral"}]},"meta":{"page":1,"total":2}}"#;
    HttpResponse {
        status: 200,
        status_text: "OK".into(),
        headers: vec![
            KeyValue::enabled("content-type", "application/json; charset=utf-8"),
            KeyValue::enabled("cache-control", "private, max-age=0"),
            KeyValue::enabled("x-request-id", "req_ferrum_preview"),
        ],
        content_type: Some("application/json".into()),
        duration: Duration::from_millis(184),
        body: ferrum_domain::ResponseBody {
            preview: body.to_vec(),
            file_path: None,
            size: u64::try_from(body.len()).unwrap_or(u64::MAX),
            truncated: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_tab_titles_without_changing_short_titles() {
        assert_eq!(truncate_title("Customers", 12), "Customers");
        assert_eq!(
            truncate_title("A very long customer request", 10),
            "A very lon…"
        );
    }

    #[test]
    fn history_status_never_relies_on_color_alone() {
        let palette = Palette::dark();
        let entry = HistoryEntry {
            id: uuid::Uuid::new_v4(),
            workspace_id: ferrum_domain::WorkspaceId::new(),
            request_id: None,
            method: HttpMethod::Get,
            url: "https://example.test".into(),
            request_headers: vec![],
            status: Some(500),
            duration_ms: Some(20),
            error: None,
            created_at: Utc::now(),
        };
        assert_eq!(history_status(&entry, palette).0, "500 Failed");
    }
}
