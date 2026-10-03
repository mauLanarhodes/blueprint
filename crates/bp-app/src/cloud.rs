//! Local official icon packs and the cloud palette. Used assets live in the document.

use crate::BlueprintApp;
use bp_icons::IconPack;
use bp_model::kurbo::Rect;
use bp_model::{CloudIcon, CloudProvider, ShapeRef};
use bp_render_egui::{Viewport, paint};
use bp_scene::{DisplayItem, DisplayList, Primitive};
use egui::{Pos2, RichText, Sense, Ui, Vec2};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

pub struct CloudState {
    pub packs: Vec<IconPack>,
    pub provider: CloudProvider,
    pub selected_version: Option<String>,
    pub manager_open: bool,
    pub terms_accepted: bool,
    pub version: String,
    pub pack_root: Option<PathBuf>,
    pub message: Option<String>,
    importing: Option<mpsc::Receiver<Result<IconPack, String>>>,
}

impl Default for CloudState {
    fn default() -> Self {
        Self {
            packs: Vec::new(),
            provider: CloudProvider::Aws,
            selected_version: None,
            manager_open: false,
            terms_accepted: false,
            version: String::new(),
            pack_root: default_pack_root(),
            message: None,
            importing: None,
        }
    }
}

fn default_pack_root() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    base.map(|base| base.join("blueprint/icon-packs"))
}

impl CloudState {
    pub(crate) fn load() -> Self {
        let mut state = Self::default();
        if let Some(root) = &state.pack_root {
            match bp_icons::load_packs(root) {
                Ok(packs) => state.packs = packs,
                Err(error) => state.message = Some(error.to_string()),
            }
        }
        state
    }

    fn active_pack(&self, provider: CloudProvider) -> Option<&IconPack> {
        self.packs.iter().rev().find(|pack| {
            pack.provider == provider
                && (provider != self.provider
                    || self
                        .selected_version
                        .as_ref()
                        .is_none_or(|version| version == &pack.version))
        })
    }
}

/// Draw the original SVG fitted into a screen rectangle.
pub(crate) fn paint_icon(
    painter: &egui::Painter,
    icon: &CloudIcon,
    rect: egui::Rect,
    opacity: f64,
) {
    if !painter.clip_rect().intersects(rect) {
        return;
    }
    let bounds = Rect::new(
        rect.min.x as f64,
        rect.min.y as f64,
        rect.max.x as f64,
        rect.max.y as f64,
    );
    let list = DisplayList {
        groups: vec![Arc::from(vec![DisplayItem {
            element: bp_model::ElementId(Default::default()),
            bbox: bounds,
            primitive: Primitive::Icon {
                svg: icon.svg.clone(),
                bounds,
                opacity,
            },
        }])],
        background: None,
    };
    paint(
        painter,
        Pos2::ZERO,
        &Viewport {
            pan: Vec2::ZERO,
            zoom: 1.0,
        },
        &list,
        None,
    );
}

impl BlueprintApp {
    pub fn cloud_icon(&self, reference: &ShapeRef) -> Option<&CloudIcon> {
        self.doc.icons.get(reference).or_else(|| {
            self.cloud
                .packs
                .iter()
                .flat_map(|pack| &pack.icons)
                .find(|icon| &icon.reference == reference)
        })
    }

    pub(crate) fn cloud_candidates(
        &self,
        provider: Option<CloudProvider>,
        query: &str,
    ) -> Vec<CloudIcon> {
        let mut icons = BTreeMap::new();
        for provider in [CloudProvider::Aws, CloudProvider::Azure]
            .into_iter()
            .filter(|candidate| provider.is_none_or(|provider| provider == *candidate))
        {
            if let Some(pack) = self.cloud.active_pack(provider) {
                for icon in bp_icons::search(pack, query.trim().trim_start_matches('/')) {
                    icons.insert(icon.reference.clone(), icon.clone());
                }
            }
        }
        let used = IconPack {
            provider: self.cloud.provider,
            version: String::new(),
            warnings: Vec::new(),
            icons: self
                .doc
                .icons
                .values()
                .filter(|icon| provider.is_none_or(|provider| provider == icon.provider))
                .cloned()
                .collect(),
        };
        for icon in bp_icons::search(&used, query.trim().trim_start_matches('/')) {
            icons.insert(icon.reference.clone(), icon.clone());
        }
        let mut result: Vec<_> = icons.into_values().collect();
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.reference.cmp(&b.reference)));
        result
    }

    pub fn open_cloud_manager(&mut self) {
        self.finish_inline_edits();
        self.cancel_drag();
        self.quick_insert = None;
        self.palette.dragging = None;
        self.cloud.manager_open = true;
    }

    pub(crate) fn poll_cloud_import(&mut self) {
        let result = self
            .cloud
            .importing
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Icon import stopped unexpectedly".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        let Some(result) = result else { return };
        self.cloud.importing = None;
        match result {
            Ok(pack) => {
                let skipped = pack.warnings.len();
                self.cloud.message = Some(format!(
                    "Imported {} {} icons{}",
                    pack.icons.len(),
                    pack.provider.label(),
                    if skipped == 0 {
                        String::new()
                    } else {
                        format!("; {skipped} entries skipped (see details below)")
                    }
                ));
                self.cloud.provider = pack.provider;
                self.cloud.selected_version = Some(pack.version.clone());
                self.cloud
                    .packs
                    .retain(|old| old.provider != pack.provider || old.version != pack.version);
                self.cloud.packs.push(pack);
            }
            Err(error) => self.cloud.message = Some(error),
        }
    }

    fn begin_cloud_import(&mut self, ctx: &egui::Context) {
        let Some(root) = self.cloud.pack_root.clone() else {
            self.cloud.message = Some("Could not find the user data directory".into());
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import official cloud icon pack")
            .add_filter("Icon pack", &["zip"])
            .pick_file()
        else {
            return;
        };
        let provider = self.cloud.provider;
        let version = self.cloud.version.trim().to_owned();
        let (sender, receiver) = mpsc::channel();
        self.cloud.importing = Some(receiver);
        self.cloud.message = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<IconPack, String> {
                let file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
                // Bound the compressed input too, before allocating a full ZIP.
                let mut bytes = Vec::new();
                use std::io::Read;
                file.take((bp_icons::MAX_ZIP_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if bytes.len() > bp_icons::MAX_ZIP_BYTES {
                    return Err("Icon ZIP exceeds 512 MiB".into());
                }
                let pack = bp_icons::import_zip(&bytes, provider, &version)
                    .map_err(|error| error.to_string())?;
                bp_icons::install_pack(&root, &pack).map_err(|error| error.to_string())?;
                Ok(pack)
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub(crate) fn cloud_pack_dialog(&mut self, ctx: &egui::Context) {
        if !self.cloud.manager_open
            || self.pending.is_some()
            || self.error.is_some()
            || self.page_choice.is_some()
        {
            return;
        }
        let busy = self.cloud.importing.is_some();
        let mut import = false;
        let modal = egui::Modal::new(egui::Id::new("cloud-icon-packs")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.heading("Cloud icon packs");
            ui.label("Download an official SVG icon ZIP, then import it here.");
            ui.add_space(10.0);
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal(|ui| {
                    for provider in [CloudProvider::Aws, CloudProvider::Azure] {
                        if ui.selectable_value(&mut self.cloud.provider, provider, provider.label()).changed() {
                            self.cloud.terms_accepted = false;
                            self.cloud.selected_version = None;
                            self.cloud.version.clear();
                            self.cloud.message = None;
                        }
                    }
                });
                let provider = self.cloud.provider;
                ui.hyperlink_to("Download official icons and read usage terms", provider.source_url());
                match provider {
                    CloudProvider::Aws => { ui.label("AWS icons are for architecture diagrams and related documentation."); }
                    CloudProvider::Azure => { ui.label("Azure icons are for architecture diagrams, training, and documentation. Keep their original appearance and display service names."); }
                }
                ui.checkbox(&mut self.cloud.terms_accepted, "I have read and accept this provider’s icon usage terms");
                ui.horizontal(|ui| {
                    ui.label("Pack release / version");
                    ui.add(egui::TextEdit::singleline(&mut self.cloud.version).hint_text("e.g. 2026-07").desired_width(150.0));
                });
                ui.label(RichText::new("Use a distinct version for each release. Existing diagrams keep their original icons.").small().weak());
                import = ui.add_enabled(self.cloud.terms_accepted && !self.cloud.version.trim().is_empty(),
                    egui::Button::new("Import ZIP…")).clicked();
            });
            if busy { ui.horizontal(|ui| { ui.spinner(); ui.label("Importing icons…"); }); }
            if let Some(message) = &self.cloud.message { ui.add_space(8.0); ui.label(message); }
            ui.separator();
            ui.label(RichText::new("Installed packs").strong());
            egui::ScrollArea::vertical().id_salt("installed-packs").max_height(140.0).show(ui, |ui| {
                if self.cloud.packs.is_empty() { ui.label(RichText::new("No packs installed").weak()); }
                for pack in &self.cloud.packs {
                    ui.label(format!("{} · {} · {} icons", pack.provider.label(), pack.version, pack.icons.len()));
                    if !pack.warnings.is_empty() {
                        egui::CollapsingHeader::new(format!("{} skipped entries", pack.warnings.len()))
                            .id_salt((pack.provider.id(), &pack.version)).show(ui, |ui| {
                                for warning in &pack.warnings { ui.label(warning); }
                            });
                    }
                }
            });
            if ui.button("Done").clicked() { self.cloud.manager_open = false; }
        });
        if modal.should_close() {
            self.cloud.manager_open = false;
        }
        if import {
            self.begin_cloud_import(ctx);
        }
    }

    pub(crate) fn cloud_palette(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for provider in [CloudProvider::Aws, CloudProvider::Azure] {
                if ui
                    .selectable_value(&mut self.cloud.provider, provider, provider.label())
                    .changed()
                {
                    self.cloud.selected_version = None;
                    self.cloud.terms_accepted = false;
                    self.cloud.version.clear();
                }
            }
        });
        if ui.button("Manage icon packs…").clicked() {
            self.open_cloud_manager();
        }
        let versions: Vec<_> = self
            .cloud
            .packs
            .iter()
            .filter(|pack| pack.provider == self.cloud.provider)
            .map(|pack| pack.version.clone())
            .collect();
        if !versions.is_empty() {
            let active = self
                .cloud
                .active_pack(self.cloud.provider)
                .map(|pack| pack.version.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("cloud-pack-version")
                .selected_text(active)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for version in versions {
                        ui.selectable_value(
                            &mut self.cloud.selected_version,
                            Some(version.clone()),
                            version,
                        );
                    }
                });
        }
        let icons = self.cloud_candidates(Some(self.cloud.provider), &self.palette.query);
        if icons.is_empty() {
            let text = if self.cloud.active_pack(self.cloud.provider).is_none() {
                "Import an official icon pack to add cloud services."
            } else {
                "No cloud icons match"
            };
            ui.label(RichText::new(text).small().weak());
        }
        let mut categories: BTreeMap<&str, Vec<&CloudIcon>> = BTreeMap::new();
        for icon in &icons {
            categories.entry(&icon.category).or_default().push(icon);
        }
        for (category, icons) in categories {
            egui::CollapsingHeader::new(category)
                .id_salt(("cloud-category", self.cloud.provider.id(), category))
                .default_open(true)
                .show(ui, |ui| {
                    let columns = ((ui.available_width() / 70.0).floor() as usize).max(1);
                    egui::Grid::new(ui.next_auto_id())
                        .spacing(Vec2::splat(2.0))
                        .show(ui, |ui| {
                            for (i, icon) in icons.iter().enumerate() {
                                if i > 0 && i % columns == 0 {
                                    ui.end_row();
                                }
                                self.cloud_cell(ui, icon);
                            }
                        });
                });
        }
        ui.separator();
    }

    fn cloud_cell(&mut self, ui: &mut Ui, icon: &CloudIcon) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(68.0, 78.0), Sense::click_and_drag());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let image = egui::Rect::from_center_size(
            rect.center_top() + Vec2::new(0.0, 28.0),
            Vec2::splat(40.0),
        );
        paint_icon(ui.painter(), icon, image, 1.0);
        let mut job = egui::text::LayoutJob::simple(
            icon.name.clone(),
            egui::FontId::proportional(10.0),
            ui.visuals().text_color(),
            rect.width() - 4.0,
        );
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = true;
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            Pos2::new(rect.center().x - galley.size().x / 2.0, rect.top() + 50.0),
            galley,
            ui.visuals().text_color(),
        );
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &icon.name));
        let response = response.on_hover_ui(|ui| {
            ui.set_max_width(260.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
            paint_icon(ui.painter(), icon, rect.shrink(4.0), 1.0);
            ui.label(RichText::new(&icon.name).strong());
            ui.label(format!(
                "{} · {} · {}",
                icon.provider.label(),
                icon.category,
                icon.pack_version
            ));
        });
        if response.clicked() {
            self.insert_shape_at(icon.reference.clone(), self.view_center());
        }
        if response.drag_started() {
            self.palette.dragging = Some(icon.reference.clone());
        }
    }
}
