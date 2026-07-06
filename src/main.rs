mod config;
mod deps;
mod queue;

use config::Config;
use eframe::egui;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use crate::deps::DepStatus;
use crate::queue::{AudioFormat, QueueItem, Source, Status};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Playlist Fetcher",
        options,
        Box::new(|cc| Ok(Box::new(PlaylistFetcherApp::new(cc)))),
    )
}

enum WorkerMsg {
    Log(usize, String),
    Finished(usize, bool, std::time::Duration, Option<String>),
    Cancelled,
}

struct PlaylistFetcherApp {
    config: Config,
    new_playlist_name: String,
    new_playlist_path: String,
    new_playlist_error: Option<String>,
    new_url: String,
    selected_playlist: Option<usize>,
    queue: Vec<QueueItem>,
    processing: bool,
    rx: Option<Receiver<WorkerMsg>>,
    last_processed: Option<String>,
    cancel_flag: Arc<AtomicBool>,
    selected_format: AudioFormat,

    // state to check if dependencies are installed so the user doesn't end up adding a bunch of
    // songs to the queue and it not work
    dep_status: DepStatus,
}

impl PlaylistFetcherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.visuals.window_corner_radius = egui::CornerRadius::same(8);
            style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
            style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
            style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, egui::Color32::from_gray(70));
            if style.visuals.dark_mode {
                style.visuals.extreme_bg_color = egui::Color32::from_rgb(28, 30, 34);
            }
        });

        Self {
            config: Config::load(),
            new_playlist_name: String::new(),
            new_playlist_path: String::new(),
            new_playlist_error: None,
            new_url: String::new(),
            selected_playlist: None,
            queue: Vec::new(),
            processing: false,
            rx: None,
            last_processed: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            selected_format: AudioFormat::Flac,
            dep_status: DepStatus::check(),
        }
    }

    fn start_processing(&mut self) {
        if self.processing || self.queue.is_empty() {
            return;
        }
        self.processing = true;
        self.cancel_flag.store(false, Ordering::SeqCst);

        for item in &mut self.queue {
            if item.status == Status::Pending {
                item.status = Status::Running;
            }
        }

        let (tx, rx): (Sender<WorkerMsg>, Receiver<WorkerMsg>) = std::sync::mpsc::channel();
        self.rx = Some(rx);

        let items = self.queue.clone();
        let playlists = self.config.playlists.clone();
        let cancel_flag = self.cancel_flag.clone();

        std::thread::spawn(move || {
            for (i, item) in items.iter().enumerate() {
                if item.status != Status::Running {
                    continue;
                }

                if cancel_flag.load(Ordering::SeqCst) {
                    let _ = tx.send(WorkerMsg::Cancelled);
                    return;
                }

                let target_dir = playlists
                    .iter()
                    .find(|p| p.name == item.playlist_name)
                    .map(|p| p.path.clone())
                    .unwrap_or_default();

                let mut cmd = match item.source {
                    Source::Spotify => {
                        let mut c = std::process::Command::new("spotdl");
                        c.arg("download")
                            .arg(&item.url)
                            .arg("--format")
                            .arg(item.format.spotdl_arg())
                            .arg("--output")
                            .arg(format!(
                                "{}/{{artist}} - {{title}}.{{output-ext}}",
                                target_dir
                            ));
                        c
                    }
                    Source::SoundCloud => {
                        let mut c = std::process::Command::new("scdl");
                        c.arg("-l")
                            .arg(&item.url)
                            .arg("--path")
                            .arg(&target_dir)
                            .arg("--name-format")
                            .arg("%(title)s");
                        c
                    }
                    Source::Unknown => continue,
                };

                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                let start = std::time::Instant::now();
                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(WorkerMsg::Log(i, format!("Failed to spawn: {e}")));
                        let _ = tx.send(WorkerMsg::Finished(i, false, start.elapsed(), None));
                        continue;
                    }
                };

                let mut was_cancelled = false;

                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => {
                            if cancel_flag.load(Ordering::SeqCst) {
                                let _ = child.kill();
                                let _ = child.wait();
                                was_cancelled = true;
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        }
                        Err(_) => break,
                    }
                }

                let elapsed = start.elapsed();
                let output = child.wait_with_output();

                match output {
                    Ok(out) => {
                        let mut log = String::from_utf8_lossy(&out.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if !stderr.is_empty() {
                            log.push_str("\n--- stderr ---\n");
                            log.push_str(&stderr);
                        }

                        // exit code alone wasn't enough to determine if a successful download or
                        // not, yt-dlp/scdl can exit 0 even when a track actually failed?
                        let error_line = log
                            .lines()
                            .find(|line| line.contains("ERROR:"))
                            .map(|line| line.trim().to_string());

                        let success =
                            !was_cancelled && out.status.success() && error_line.is_none();

                        let _ = tx.send(WorkerMsg::Log(i, log));
                        let _ = tx.send(WorkerMsg::Finished(i, success, elapsed, error_line));
                    }
                    Err(e) => {
                        let _ = tx.send(WorkerMsg::Log(i, format!("Error: {e}")));
                        let _ =
                            tx.send(WorkerMsg::Finished(i, false, elapsed, Some(e.to_string())));
                    }
                }

                if was_cancelled {
                    let _ = tx.send(WorkerMsg::Cancelled);
                    return;
                }
            }
        });
    }
}

impl eframe::App for PlaylistFetcherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMsg::Log(i, log) => {
                        println!("[{i}] {log}");
                    }
                    WorkerMsg::Finished(i, success, elapsed, error) => {
                        if let Some(item) = self.queue.get_mut(i) {
                            item.status = if success {
                                Status::Done
                            } else {
                                Status::Failed
                            };
                            item.error = error;
                            let verb = if success { "Processed" } else { "Failed" };
                            self.last_processed = Some(format!(
                                "{verb} {} — took {:.1}s",
                                item.url,
                                elapsed.as_secs_f32()
                            ));
                        }
                    }
                    WorkerMsg::Cancelled => {
                        for item in &mut self.queue {
                            if item.status == Status::Running {
                                item.status = Status::Pending;
                            }
                        }
                        self.last_processed = Some("Processing stopped".to_string());
                        self.processing = false;
                    }
                }
            }

            if self
                .queue
                .iter()
                .all(|item| item.status != Status::Pending && item.status != Status::Running)
            {
                self.processing = false;
            }
        }

        egui::Panel::left("playlists_panel")
            .resizable(false)
            .exact_size(240.0)
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(18.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Playlists").heading().strong());
                ui.add_space(4.0);
                ui.add_space(8.0);

                if self.config.playlists.is_empty() {
                    ui.label(egui::RichText::new("No playlists yet").weak().italics());
                    ui.add_space(8.0);
                } else {
                    let mut to_remove: Option<usize> = None;
                    for (i, playlist) in self.config.playlists.iter().enumerate() {
                        egui::Frame::default()
                            .fill(ui.visuals().faint_bg_color)
                            .corner_radius(8)
                            .inner_margin(10.0)
                            .stroke(egui::Stroke::NONE)
                            .shadow(egui::Shadow::NONE)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(&playlist.name).strong());
                                        ui.label(
                                            egui::RichText::new(&playlist.path).small().weak(),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let resp = ui.small_button("×");
                                            if resp
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                to_remove = Some(i);
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(6.0);
                    }
                    if let Some(i) = to_remove {
                        self.config.playlists.remove(i);
                        self.config.save();
                        self.selected_playlist = None;
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(egui::RichText::new("New playlist").strong());
                ui.add_space(4.0);

                ui.add(
                    egui::TextEdit::singleline(&mut self.new_playlist_name)
                        .hint_text("Playlist name")
                        .margin(egui::Margin::symmetric(10, 8))
                        .desired_width(ui.available_width()),
                );

                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_playlist_path)
                            .hint_text("Folder path")
                            .margin(egui::Margin::symmetric(10, 8))
                            .desired_width(ui.available_width() - 36.0),
                    );
                    let picker_resp = ui.button("...");
                    if picker_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.new_playlist_path = folder.display().to_string();
                        }
                    }
                });

                if let Some(err) = &self.new_playlist_error {
                    ui.add_space(4.0);
                    ui.colored_label(egui::Color32::from_rgb(230, 90, 90), err);
                }

                ui.add_space(6.0);

                let add_resp = ui.add_sized([ui.available_width(), 32.0], egui::Button::new("Add"));
                if add_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    let name = self.new_playlist_name.trim();
                    let path = self.new_playlist_path.trim();

                    if name.is_empty() || path.is_empty() {
                        self.new_playlist_error = Some("Name and path are required".into());
                    } else if self
                        .config
                        .playlists
                        .iter()
                        .any(|p| p.name.eq_ignore_ascii_case(name))
                    {
                        self.new_playlist_error =
                            Some("A playlist with that name already exists".into());
                    } else if !std::path::Path::new(path).is_dir() {
                        self.new_playlist_error = Some("That folder doesn't exist".into());
                    } else {
                        self.config.playlists.push(config::Playlist {
                            name: name.to_string(),
                            path: path.to_string(),
                        });
                        self.config.save();
                        self.new_playlist_name.clear();
                        self.new_playlist_path.clear();
                        self.new_playlist_error = None;
                    }
                }

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Dependencies").strong());
                    let recheck_btn = ui.small_button("Recheck");
                    if recheck_btn
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.dep_status = DepStatus::check();
                    }
                });

                let dep_check = |ui: &mut egui::Ui, name: &str, ok: bool| {
                    ui.horizontal(|ui| {
                        let (label, color) = if ok {
                            ("Installed", egui::Color32::from_rgb(100, 200, 120))
                        } else {
                            ("Missing", egui::Color32::from_rgb(230, 90, 90))
                        };

                        ui.label(format!("{}:", name));
                        ui.label(egui::RichText::new(label).color(color).strong());
                    })
                };

                dep_check(ui, "spotdl", self.dep_status.spotdl);
                dep_check(ui, "scdl", self.dep_status.scdl);
                dep_check(ui, "ffmpeg", self.dep_status.ffmpeg);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(20.0))
            .show(ui, |ui| {
                egui::Panel::bottom("process_bar")
                    .resizable(false)
                    .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(0, 12)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;

                            ui.add_enabled_ui(!self.queue.is_empty() && !self.processing, |ui| {
                                let resp =
                                    ui.add_sized([120.0, 32.0], egui::Button::new("Process Queue"));
                                if resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.start_processing();
                                }
                            });

                            if self.processing {
                                let stop_resp =
                                    ui.add_sized([70.0, 32.0], egui::Button::new("Stop"));
                                if stop_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.cancel_flag.store(true, Ordering::SeqCst);
                                }
                                // ui.add(egui::Spinner::new().size(16.0));
                            }

                            let total = self.queue.len().max(1);
                            let done = self
                                .queue
                                .iter()
                                .filter(|i| matches!(i.status, Status::Done | Status::Failed))
                                .count();

                            if self.processing {
                                ui.add(
                                    egui::ProgressBar::new(done as f32 / total as f32)
                                        .text(format!("{done}/{total}"))
                                        .desired_width(ui.available_width())
                                        .desired_height(24.0),
                                );
                            }
                        });
                        if let Some(msg) = &self.last_processed {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(msg).small().weak());
                        }
                    });

                ui.heading("Add to Queue");

                ui.add(
                    egui::TextEdit::singleline(&mut self.new_url)
                        .hint_text("Paste a Spotify or SoundCloud URL")
                        .margin(egui::Margin::symmetric(10, 8))
                        .desired_width(ui.available_width()),
                );

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Target Playlist");
                        egui::ComboBox::from_id_salt("target_playlist_combo")
                            .selected_text(
                                self.selected_playlist
                                    .and_then(|i| self.config.playlists.get(i))
                                    .map(|p| p.name.as_str())
                                    .unwrap_or("Select..."),
                            )
                            .show_ui(ui, |ui| {
                                for (i, playlist) in self.config.playlists.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.selected_playlist,
                                        Some(i),
                                        &playlist.name,
                                    );
                                }
                            });
                    });

                    let no_format_option = Source::from_url(&self.new_url) == Source::SoundCloud
                        || Source::from_url(&self.new_url) == Source::Unknown;

                    ui.vertical(|ui| {
                        ui.label("Format");
                        ui.add_enabled_ui(!no_format_option, |ui| {
                            egui::ComboBox::from_id_salt("format_combo")
                                .selected_text(self.selected_format.label())
                                .show_ui(ui, |ui| {
                                    for fmt in AudioFormat::ALL {
                                        ui.selectable_value(
                                            &mut self.selected_format,
                                            fmt,
                                            fmt.label(),
                                        );
                                    }
                                });
                        })
                        .response
                        .on_disabled_hover_text(
                            "Current link does not support switching format options.",
                        );
                    });
                    let invalid_option = Source::from_url(&self.new_url) == Source::Unknown;

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::BOTTOM), |ui| {
                        ui.add_enabled_ui(!invalid_option, |ui| {
                            let resp = ui.button("Add to Queue");
                            if resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                if let Some(id) = self.selected_playlist {
                                    let source = Source::from_url(&self.new_url);
                                    if source != Source::Unknown && !self.new_url.is_empty() {
                                        self.queue.push(QueueItem {
                                            url: self.new_url.clone(),
                                            playlist_name: self.config.playlists[id].name.clone(),
                                            source,
                                            status: Status::Pending,
                                            format: self.selected_format,
                                            error: None,
                                        });
                                        self.new_url.clear();
                                    }
                                }
                            }
                        })
                        .response
                        .on_disabled_hover_text("Invalid link");
                    });
                });

                ui.separator();
                ui.label("Queue:");

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut to_remove: Option<usize> = None;

                        for (i, item) in self.queue.iter().enumerate() {
                            let running = item.status == Status::Running;

                            let mut frame = egui::Frame::default()
                                .fill(if running {
                                    egui::Color32::from_rgb(30, 45, 65)
                                } else {
                                    ui.visuals().faint_bg_color
                                })
                                .corner_radius(6)
                                .inner_margin(10.0)
                                .stroke(egui::Stroke::NONE)
                                .shadow(egui::Shadow::NONE);

                            if running {
                                frame = frame.stroke(egui::Stroke::new(
                                    1.5,
                                    egui::Color32::from_rgb(80, 160, 255),
                                ));
                            }

                            frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(item.source.label()).strong());
                                    if item.source == Source::Spotify {
                                        ui.label(egui::RichText::new(item.format.label()).weak());
                                    }
                                    let url_resp = ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&item.url).underline(),
                                        )
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                    );
                                    if url_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        let _ = open::that(&item.url);
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.add_enabled_ui(!running, |ui| {
                                                let resp = ui.small_button("×");
                                                if resp
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    to_remove = Some(i);
                                                }
                                            });
                                        },
                                    );
                                });
                                ui.horizontal(|ui| {
                                    let (text, color) = match item.status {
                                        Status::Pending => ("Pending", egui::Color32::GRAY),
                                        Status::Running => {
                                            ("Running", egui::Color32::from_rgb(80, 160, 255))
                                        }
                                        Status::Done => {
                                            ("Done", egui::Color32::from_rgb(100, 200, 120))
                                        }
                                        Status::Failed => {
                                            ("Failed", egui::Color32::from_rgb(230, 90, 90))
                                        }
                                    };
                                    ui.label(egui::RichText::new(text).color(color).strong());
                                    ui.label(egui::RichText::new(&item.playlist_name).weak());
                                });
                                if item.status == Status::Failed {
                                    if let Some(err) = &item.error {
                                        ui.label(
                                            egui::RichText::new(err)
                                                .small()
                                                .color(egui::Color32::from_rgb(230, 90, 90)),
                                        );
                                    }
                                }
                            });
                            ui.add_space(4.0);
                        }

                        if let Some(i) = to_remove {
                            self.queue.remove(i);
                        }
                    });
            });
    }
}
