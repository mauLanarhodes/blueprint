//! Local official icon packs and the cloud palette. Used assets live in the document.

use crate::BlueprintApp;
use crate::preferences::CatalogView;
use bp_icons::IconPack;
use bp_model::kurbo::Rect;
use bp_model::{CloudIcon, CloudProvider, ShapeRef};
use bp_render_egui::{Viewport, paint};
use bp_scene::{DisplayItem, DisplayList, Primitive};
use egui::{Pos2, RichText, Sense, Ui, Vec2};
use egui_phosphor::regular as glyph;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
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
    calendar: crate::calendar::Calendar,
    candidates: RefCell<Vec<CandidateCache>>,
    lookup: RefCell<CatalogLookup>,
}

#[derive(Default)]
struct CatalogLookup {
    source: Option<u64>,
    positions: HashMap<ShapeRef, (usize, usize)>,
}

#[derive(PartialEq)]
struct CandidateKey {
    source: u64,
    provider: Option<CloudProvider>,
    version: Option<String>,
    query: String,
    category: Option<String>,
    list: bool,
}

struct CandidateCache {
    key: CandidateKey,
    icons: Arc<[CloudIcon]>,
    categories: Arc<[String]>,
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
            calendar: crate::calendar::Calendar::default(),
            candidates: RefCell::default(),
            lookup: RefCell::default(),
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
    fn import_disabled_reason(&self) -> Option<&'static str> {
        if !self.terms_accepted {
            return Some("Check the terms box above to enable importing.");
        }
        let version = self.version.trim();
        if version.is_empty() {
            return Some(
                "Enter a pack version above to enable importing. The grey example is a placeholder.",
            );
        }
        if version.len() > 64
            || matches!(version, "." | "..")
            || !version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Some("Use 1–64 letters, digits, dots, underscores or hyphens for the version.");
        }
        None
    }

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

    /// Saved references retain their original artwork version. A document's
    /// embedded asset is sufficient even when that catalog is not installed.
    fn cloud_saved_icons(&self, references: &[ShapeRef]) -> (Vec<CloudIcon>, Vec<ShapeRef>) {
        let mut source = std::collections::hash_map::DefaultHasher::new();
        for pack in &self.cloud.packs {
            pack.provider.id().hash(&mut source);
            pack.version.hash(&mut source);
            pack.icons.len().hash(&mut source);
            pack.icons.as_ptr().hash(&mut source);
        }
        let source = source.finish();
        let mut lookup = self.cloud.lookup.borrow_mut();
        if lookup.source != Some(source) {
            lookup.positions.clear();
            for (pack_index, pack) in self.cloud.packs.iter().enumerate() {
                for (icon_index, icon) in pack.icons.iter().enumerate() {
                    lookup
                        .positions
                        .entry(icon.reference.clone())
                        .or_insert((pack_index, icon_index));
                }
            }
            lookup.source = Some(source);
        }
        let mut icons = Vec::new();
        let mut unavailable = Vec::new();
        for reference in references
            .iter()
            .filter(|reference| reference.library() == self.cloud.provider.id())
        {
            let icon = self.doc.icons.get(reference).or_else(|| {
                lookup
                    .positions
                    .get(reference)
                    .map(|&(pack, icon)| &self.cloud.packs[pack].icons[icon])
            });
            if let Some(icon) = icon {
                icons.push(icon.clone());
            } else {
                unavailable.push(reference.clone());
            }
        }
        (icons, unavailable)
    }

    fn validate_cloud_category(&mut self) {
        let Some(category) = self.preferences.cloud_category.as_ref() else {
            return;
        };
        if !self
            .cloud_candidates(Some(self.cloud.provider), "")
            .iter()
            .any(|icon| &icon.category == category)
        {
            self.preferences.cloud_category = None;
        }
    }

    pub(crate) fn cloud_candidates(
        &self,
        provider: Option<CloudProvider>,
        query: &str,
    ) -> Arc<[CloudIcon]> {
        let mut source = std::collections::hash_map::DefaultHasher::new();
        // Artwork is immutable; moving or editing diagram elements should not
        // rebuild a whole installed catalog during normal canvas interaction.
        self.cloud.provider.id().hash(&mut source);
        for pack in &self.cloud.packs {
            pack.provider.id().hash(&mut source);
            pack.version.hash(&mut source);
            pack.icons.len().hash(&mut source);
            pack.icons.as_ptr().hash(&mut source);
        }
        for (reference, icon) in &self.doc.icons {
            reference.hash(&mut source);
            icon.svg.as_ptr().hash(&mut source);
        }
        let key = CandidateKey {
            source: source.finish(),
            provider,
            version: self.cloud.selected_version.clone(),
            query: query.trim().trim_start_matches('/').to_owned(),
            category: self.preferences.cloud_category.clone(),
            list: self.preferences.cloud_view == CatalogView::List,
        };
        if let Some(cached) = self.cloud.candidates.borrow().iter().find(|c| c.key == key) {
            return cached.icons.clone();
        }
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
        let icons: Arc<[CloudIcon]> = result.into();
        let categories: Arc<[String]> = icons
            .iter()
            .map(|icon| icon.category.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into();
        let mut cache = self.cloud.candidates.borrow_mut();
        cache.retain(|entry| entry.key.source == key.source);
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push(CandidateCache {
            key,
            icons: icons.clone(),
            categories,
        });
        icons
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
                self.preferences.cloud_provider = pack.provider;
                self.preferences.cloud_version = Some(pack.version.clone());
                self.cloud
                    .packs
                    .retain(|old| old.provider != pack.provider || old.version != pack.version);
                self.cloud.packs.push(pack);
                self.validate_cloud_category();
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
            ui.set_width(480.0_f32.min((ctx.content_rect().width() - 48.0).max(240.0)));
            ui.heading("Cloud icon packs");
            egui::ScrollArea::vertical().id_salt("cloud-pack-setup")
                .max_height((ctx.content_rect().height() - 130.0).max(180.0))
                .show(ui, |ui| {
            ui.label("Download an official SVG icon ZIP, then import it here.");
            ui.add_space(10.0);
            ui.add_enabled_ui(!busy, |ui| {
                ui.label(RichText::new("1. Choose a provider and download its ZIP").strong());
                ui.horizontal(|ui| {
                    for provider in [CloudProvider::Aws, CloudProvider::Azure] {
                        if ui.selectable_value(&mut self.cloud.provider, provider, provider.label()).changed() {
                            self.cloud.terms_accepted = false;
                            self.cloud.selected_version = None;
                            self.cloud.version.clear();
                            self.cloud.message = None;
                            self.preferences.cloud_provider = self.cloud.provider;
                            self.preferences.cloud_version = None;
                            self.preferences.cloud_category = None;
                        }
                    }
                });
                let provider = self.cloud.provider;
                ui.hyperlink_to("Download official icons and read usage terms", provider.source_url());
                match provider {
                    CloudProvider::Aws => { ui.label("AWS icons are for architecture diagrams and related documentation."); }
                    CloudProvider::Azure => { ui.label("Azure icons are for architecture diagrams, training, and documentation. Keep their original appearance and display service names."); }
                }
                ui.add_space(6.0);
                ui.label(RichText::new("2. Confirm the terms and pack version").strong());
                ui.checkbox(&mut self.cloud.terms_accepted, "I have read and accept this provider’s icon usage terms");
                ui.horizontal(|ui| {
                    let label = ui.label("Pack version (required)");
                    ui.add(crate::widgets::text_field(&mut self.cloud.version).hint_text("e.g. 2026-07-31").desired_width(150.0))
                        .labelled_by(label.id);
                    self.cloud.calendar.version_picker(ui, &mut self.cloud.version);
                });
                ui.label(RichText::new("Choose the pack’s release date with the calendar, or type a version such as v24. Existing diagrams keep their original icons.").small().weak());
                ui.add_space(6.0);
                ui.label(RichText::new("3. Import the original SVG artwork").strong());
                let reason = self.cloud.import_disabled_reason();
                let button = ui.add_enabled(reason.is_none(), egui::Button::new("Import ZIP…"));
                if let Some(reason) = reason {
                    button.on_disabled_hover_text(reason);
                    ui.label(reason);
                } else {
                    import = button.clicked();
                }
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

    pub(crate) fn cloud_palette(&mut self, ui: &mut Ui, viewport: egui::Rect) {
        ui.horizontal(|ui| {
            for provider in [CloudProvider::Aws, CloudProvider::Azure] {
                let response =
                    ui.selectable_value(&mut self.cloud.provider, provider, provider.label());
                crate::widgets::focus_outline(ui, &response);
                crate::widgets::command_focus(ui, &response);
                if response.changed() {
                    self.cloud.selected_version = None;
                    self.cloud.terms_accepted = false;
                    self.cloud.version.clear();
                    self.preferences.cloud_provider = self.cloud.provider;
                    self.preferences.cloud_version = None;
                    self.preferences.cloud_category = None;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let list = crate::widgets::icon_button(ui, glyph::LIST, "Service list view");
                list.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        self.preferences.cloud_view == CatalogView::List,
                        "Service list view",
                    )
                });
                if self.preferences.cloud_view == CatalogView::List {
                    ui.painter().rect_stroke(
                        list.rect.shrink(1.0),
                        4.0,
                        egui::Stroke::new(1.0, crate::theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                if list.clicked() {
                    self.preferences.cloud_view = CatalogView::List;
                }
                let grid = crate::widgets::icon_button(ui, glyph::GRID_FOUR, "Service grid view");
                grid.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        self.preferences.cloud_view == CatalogView::Grid,
                        "Service grid view",
                    )
                });
                if self.preferences.cloud_view == CatalogView::Grid {
                    ui.painter().rect_stroke(
                        grid.rect.shrink(1.0),
                        4.0,
                        egui::Stroke::new(1.0, crate::theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                if grid.clicked() {
                    self.preferences.cloud_view = CatalogView::Grid;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let response = ui.button("Manage icon packs…");
            crate::widgets::focus_outline(ui, &response);
            crate::widgets::command_focus(ui, &response);
            if response.clicked() {
                self.open_cloud_manager();
            }
        });
        if let Some(message) = &self.cloud.message {
            let header = egui::CollapsingHeader::new("Icon pack message")
                .id_salt("cloud-pack-notice")
                .show(ui, |ui| {
                    ui.label(message);
                });
            crate::widgets::focus_outline(ui, &header.header_response);
            crate::widgets::command_focus(ui, &header.header_response);
        }
        if self.cloud.provider == CloudProvider::Aws {
            let trial = ui.small_button("Add AWS trial stack");
            crate::widgets::focus_outline(ui, &trial);
            crate::widgets::command_focus(ui, &trial);
            if trial.clicked() {
                self.add_aws_trial();
            }
            let response = ui.small_button("Add starter network");
            crate::widgets::focus_outline(ui, &response);
            crate::widgets::command_focus(ui, &response);
            if response.clicked() {
                self.add_starter_network();
            }
        }
        let versions: Vec<_> = self
            .cloud
            .packs
            .iter()
            .filter(|pack| pack.provider == self.cloud.provider)
            .map(|pack| pack.version.clone())
            .collect();
        if !versions.is_empty() || self.cloud.selected_version.is_some() {
            let active = self
                .cloud
                .active_pack(self.cloud.provider)
                .map(|pack| pack.version.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{} (unavailable)",
                        self.cloud
                            .selected_version
                            .as_deref()
                            .unwrap_or("Selected version")
                    )
                });
            let version_label = ui.label(RichText::new("Icon pack version").small().weak());
            let response = egui::ComboBox::from_id_salt("cloud-pack-version")
                .selected_text(active)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    let response = ui.selectable_value(
                        &mut self.cloud.selected_version,
                        None,
                        "Default installed version",
                    );
                    crate::widgets::command_focus(ui, &response);
                    for version in versions {
                        let response = ui.selectable_value(
                            &mut self.cloud.selected_version,
                            Some(version.clone()),
                            version,
                        );
                        crate::widgets::command_focus(ui, &response);
                    }
                })
                .response
                .labelled_by(version_label.id);
            crate::widgets::focus_outline(ui, &response);
            crate::widgets::command_focus(ui, &response);
            self.preferences.cloud_version = self.cloud.selected_version.clone();
            if self.cloud.active_pack(self.cloud.provider).is_none() {
                ui.label(RichText::new("This pack version is unavailable. Choose an installed version or import its original ZIP.").small().weak());
            }
        }
        self.validate_cloud_category();
        let available = self.cloud_candidates(Some(self.cloud.provider), "");
        let categories = self
            .cloud
            .candidates
            .borrow()
            .iter()
            .find(|entry| Arc::ptr_eq(&entry.icons, &available))
            .map(|entry| entry.categories.clone())
            .unwrap_or_default();
        let response = egui::ComboBox::from_id_salt("cloud-category-filter")
            .selected_text(
                self.preferences
                    .cloud_category
                    .as_deref()
                    .unwrap_or("All categories"),
            )
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                let response = ui.selectable_value(
                    &mut self.preferences.cloud_category,
                    None,
                    "All categories",
                );
                crate::widgets::command_focus(ui, &response);
                for category in categories.iter() {
                    let response = ui.selectable_value(
                        &mut self.preferences.cloud_category,
                        Some(category.clone()),
                        category,
                    );
                    crate::widgets::command_focus(ui, &response);
                }
            })
            .response;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Service category")
        });
        crate::widgets::focus_outline(ui, &response);
        crate::widgets::command_focus(ui, &response);

        if self.palette.query.trim().is_empty() && self.preferences.cloud_category.is_none() {
            let (favorite_icons, missing) = self.cloud_saved_icons(&self.preferences.favorites);
            if !favorite_icons.is_empty() || !missing.is_empty() {
                ui.label(RichText::new("Favorites").small().strong());
                self.cloud_rows(
                    ui,
                    &favorite_icons.iter().collect::<Vec<_>>(),
                    viewport,
                    "favorites",
                );
                for reference in missing {
                    ui.horizontal(|ui| {
                        let remove = crate::widgets::icon_button(
                            ui,
                            glyph::X,
                            "Remove unavailable favorite",
                        );
                        ui.add(
                            egui::Label::new(RichText::new(reference.as_str()).small().weak())
                                .truncate(),
                        )
                        .on_hover_text(reference.as_str());
                        if remove.clicked() {
                            self.preferences.favorites.retain(|item| item != &reference);
                        }
                    });
                    ui.label(
                        RichText::new("Install this icon’s original pack version to use it again.")
                            .small()
                            .weak(),
                    );
                }
            }
            let (recent, _) = self.cloud_saved_icons(&self.palette.recent);
            if !recent.is_empty() {
                ui.label(RichText::new("Recent services").small().strong());
                self.cloud_rows(ui, &recent.iter().collect::<Vec<_>>(), viewport, "recent");
            }
        }
        let icons = self.cloud_candidates(Some(self.cloud.provider), &self.palette.query);
        let icons: Vec<_> = icons
            .iter()
            .filter(|icon| {
                self.preferences
                    .cloud_category
                    .as_ref()
                    .is_none_or(|category| category == &icon.category)
            })
            .collect();
        ui.label(
            RichText::new(format!("Services ({})", icons.len()))
                .small()
                .strong(),
        );
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
            let header = egui::CollapsingHeader::new(category)
                .id_salt(("cloud-category", self.cloud.provider.id(), category))
                .default_open(true)
                .show(ui, |ui| {
                    self.cloud_rows(ui, &icons, viewport, category);
                });
            crate::widgets::focus_outline(ui, &header.header_response);
            crate::widgets::command_focus(ui, &header.header_response);
        }
        ui.separator();
    }

    fn cloud_rows(
        &mut self,
        ui: &mut Ui,
        icons: &[&CloudIcon],
        viewport: egui::Rect,
        section: &str,
    ) {
        let width = ui.available_width();
        let list = self.preferences.cloud_view == CatalogView::List;
        let columns = if list {
            1
        } else {
            ((width + 6.0) / 104.0).floor().max(1.0) as usize
        };
        let size = Vec2::new(
            (width - (columns - 1) as f32 * 6.0) / columns as f32,
            if list { 56.0 } else { 100.0 },
        );
        let rows = icons.len().div_ceil(columns);
        if rows == 0 {
            return;
        }
        let (_, content) = ui.allocate_space(Vec2::new(width, rows as f32 * (size.y + 6.0) - 6.0));
        let start = ((viewport.min.y - content.min.y) / (size.y + 6.0))
            .floor()
            .max(0.0) as usize;
        let end = (((viewport.max.y - content.min.y) / (size.y + 6.0))
            .ceil()
            .max(0.0) as usize
            + 1)
        .min(rows);
        // One adjacent row keeps keyboard traversal continuous while the rest
        // of a large catalog stays outside the widget/rendering work.
        for row in start.saturating_sub(1).min(rows)..end {
            for column in 0..columns {
                let Some(icon) = icons.get(row * columns + column) else {
                    break;
                };
                let rect = egui::Rect::from_min_size(
                    content.min
                        + Vec2::new(column as f32 * (size.x + 6.0), row as f32 * (size.y + 6.0)),
                    size,
                );
                let mut cell = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("cloud-cell", section, &icon.reference))
                        .max_rect(rect),
                );
                self.cloud_cell(&mut cell, icon, size, list);
            }
        }
    }

    fn cloud_cell(&mut self, ui: &mut Ui, icon: &CloudIcon, size: Vec2, list: bool) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        // The later favorite button owns its small overlapping hit rectangle;
        // the rest of the visible cell, including its label, inserts the icon.
        let response = ui.interact(rect, ui.id().with("insert"), Sense::click_and_drag());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let image = if list {
            egui::Rect::from_min_size(rect.min + Vec2::new(6.0, 12.0), Vec2::splat(32.0))
        } else {
            egui::Rect::from_center_size(
                rect.center_top() + Vec2::new(-6.0, 29.0),
                Vec2::splat(40.0),
            )
        };
        paint_icon(ui.painter(), icon, image, 1.0);
        let mut job = egui::text::LayoutJob::simple(
            icon.name.clone(),
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
            rect.width() - 4.0,
        );
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = false;
        if list {
            job.wrap.max_width = (rect.width() - 74.0).max(1.0);
        }
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            if list {
                Pos2::new(image.max.x + 8.0, rect.center().y - galley.size().y / 2.0)
            } else {
                Pos2::new(rect.center().x - galley.size().x / 2.0, rect.top() + 55.0)
            },
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
            ui.label(RichText::new(&icon.source_path).small().weak());
            ui.label(format!(
                "{} · {} · {}",
                icon.provider.label(),
                icon.category,
                icon.pack_version
            ));
            ui.label("Diagram artwork. Deployment requires explicit supported resource settings.");
        });
        crate::widgets::focus_outline(ui, &response);
        crate::widgets::command_focus(ui, &response);
        if response.gained_focus()
            && !ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released())
        {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        let favorite = self.preferences.favorites.contains(&icon.reference);
        let star = egui::Rect::from_min_size(
            Pos2::new(rect.max.x - 30.0, rect.min.y + 2.0),
            Vec2::splat(28.0),
        );
        let label = if favorite {
            format!("Remove {} from favorites", icon.name)
        } else {
            format!("Favorite {}", icon.name)
        };
        let favorite_button = ui
            .put(
                star,
                egui::Button::new(crate::theme::icon(glyph::STAR))
                    .frame(false)
                    .selected(favorite),
            )
            .on_hover_text(&label);
        favorite_button
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        crate::widgets::focus_outline(ui, &favorite_button);
        crate::widgets::command_focus(ui, &favorite_button);
        if favorite_button.gained_focus()
            && !ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released())
        {
            favorite_button.scroll_to_me(Some(egui::Align::Center));
        }
        if favorite_button.clicked() {
            if favorite {
                self.preferences
                    .favorites
                    .retain(|item| item != &icon.reference);
            } else {
                self.preferences.favorites.push(icon.reference.clone());
            }
        }
        if response.clicked() && !favorite_button.clicked() {
            self.insert_shape_at(icon.reference.clone(), self.view_center());
        }
        if response.drag_started() && !favorite_button.is_pointer_button_down_on() {
            self.palette.dragging = Some(icon.reference.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    fn pack(provider: CloudProvider, version: &str) -> IconPack {
        let name = if provider == CloudProvider::Aws {
            "Amazon EC2"
        } else {
            "Virtual Machines"
        };
        IconPack {
            provider,
            version: version.into(),
            warnings: Vec::new(),
            icons: vec![CloudIcon {
                reference: ShapeRef::new(
                    provider.id(),
                    &format!("compute-service-sample@{version}"),
                ),
                name: name.into(),
                provider,
                category: "Compute".into(),
                kind: bp_model::IconKind::Service,
                pack_version: version.into(),
                source_path: "Compute/Sample.svg".into(),
                svg: Arc::from(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="blue"/></svg>"#,
                ),
            }],
        }
    }

    fn harness(
        preferences: crate::preferences::WorkspacePreferences,
        catalogs: Vec<IconPack>,
    ) -> Harness<'static, Option<BlueprintApp>> {
        let mut harness = Harness::builder()
            .with_size(Vec2::new(1360.0, 860.0))
            .with_step_dt(1.0 / 60.0)
            .build_ui_state(
                move |ui, app: &mut Option<BlueprintApp>| {
                    let app = app.get_or_insert_with(|| {
                        let mut app = BlueprintApp::new_with_workspace(
                            ui.ctx(),
                            None,
                            preferences.clone(),
                            catalogs.clone(),
                        );
                        app.choose_page_kind(bp_model::DiagramKind::Cloud);
                        app
                    });
                    app.frame(ui);
                },
                None,
            );
        harness.run();
        harness
    }

    #[test]
    fn unavailable_version_is_visible_without_installed_packs() {
        let mut harness = harness(
            crate::preferences::WorkspacePreferences {
                cloud_version: Some("missing-v1".into()),
                cloud_category: Some("Retired category".into()),
                ..Default::default()
            },
            Vec::new(),
        );
        assert!(harness.query_by_label("This pack version is unavailable. Choose an installed version or import its original ZIP.").is_some());
        let app = harness.state_mut().as_mut().unwrap();
        assert_eq!(app.cloud.selected_version.as_deref(), Some("missing-v1"));
        assert!(app.preferences.cloud_category.is_none());
        assert!(app.doc.elements.is_empty());
    }

    #[test]
    fn category_is_revalidated_for_version_changes_and_completed_imports() {
        let mut storage = pack(CloudProvider::Aws, "v2");
        storage.icons[0].category = "Storage".into();
        let mut app = BlueprintApp::new_with_workspace(
            &egui::Context::default(),
            None,
            crate::preferences::WorkspacePreferences {
                cloud_version: Some("v1".into()),
                cloud_category: Some("Compute".into()),
                ..Default::default()
            },
            vec![pack(CloudProvider::Aws, "v1"), storage],
        );
        app.validate_cloud_category();
        assert_eq!(app.preferences.cloud_category.as_deref(), Some("Compute"));
        app.cloud.selected_version = Some("v2".into());
        app.validate_cloud_category();
        assert!(app.preferences.cloud_category.is_none());
        app.preferences.cloud_category = Some("Storage".into());
        let before = app.doc.clone();
        let (sender, receiver) = mpsc::channel();
        app.cloud.importing = Some(receiver);
        sender.send(Ok(pack(CloudProvider::Aws, "v3"))).unwrap();
        app.poll_cloud_import();
        assert_eq!(app.cloud.selected_version.as_deref(), Some("v3"));
        assert!(app.preferences.cloud_category.is_none());
        assert_eq!(app.doc, before);
    }

    #[test]
    fn whole_catalog_cell_inserts_while_its_star_only_changes_preferences() {
        let mut harness = harness(Default::default(), vec![pack(CloudProvider::Aws, "v1")]);
        harness.get_by_label("Favorite Amazon EC2").click();
        harness.run();
        assert!(harness.state().as_ref().unwrap().doc.elements.is_empty());
        let cell = harness
            .get_all_by_label("Amazon EC2")
            .rfind(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap()
            .rect();
        let position = Pos2::new(cell.max.x - 4.0, cell.max.y - 10.0);
        harness.hover_at(position);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
        }
        let app = harness.state().as_ref().unwrap();
        assert_eq!(app.doc.elements.len(), 1);
        assert_eq!(app.preferences.favorites.len(), 1);
    }

    #[test]
    fn provider_and_category_focus_allow_undo_while_search_focus_keeps_document() {
        let catalog = pack(CloudProvider::Aws, "v1");
        let reference = catalog.icons[0].reference.clone();
        let mut harness = harness(Default::default(), vec![catalog]);
        let before = harness.state().as_ref().unwrap().doc.clone();
        for label in ["AWS", "Service category"] {
            harness
                .state_mut()
                .as_mut()
                .unwrap()
                .insert_shape_at(reference.clone(), bp_model::kurbo::Point::ZERO)
                .unwrap();
            harness.run();
            harness.get_by_label(label).focus();
            harness.run();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
            harness.run();
            assert_eq!(
                harness.state().as_ref().unwrap().doc,
                before,
                "{label} focus must allow document undo"
            );
        }
        harness
            .state_mut()
            .as_mut()
            .unwrap()
            .insert_shape_at(reference, bp_model::kurbo::Point::ZERO)
            .unwrap();
        harness.run();
        harness.get_by_label("Search shapes and services").focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(harness.state().as_ref().unwrap().doc.elements.len(), 1);
    }

    #[test]
    fn search_refreshes_after_catalog_changes_and_retains_embedded_originals() {
        let mut app = BlueprintApp::new_with_workspace(
            &egui::Context::default(),
            None,
            Default::default(),
            vec![
                pack(CloudProvider::Aws, "v1"),
                pack(CloudProvider::Azure, "v1"),
            ],
        );
        let first = app.cloud_candidates(Some(CloudProvider::Aws), "vm");
        assert_eq!(first.len(), 1);
        assert_eq!(
            app.cloud_candidates(Some(CloudProvider::Azure), "vm")[0].name,
            "Virtual Machines"
        );
        app.doc
            .icons
            .insert(first[0].reference.clone(), first[0].clone());
        app.cloud.packs[0] = pack(CloudProvider::Aws, "v2");
        let refreshed = app.cloud_candidates(Some(CloudProvider::Aws), "vm");
        assert_eq!(refreshed.len(), 2);
        assert!(refreshed.iter().any(|icon| icon.pack_version == "v2"));
        app.cloud.packs.clear();
        let embedded = app.cloud_candidates(Some(CloudProvider::Aws), "vm");
        assert_eq!(embedded.len(), 1);
        assert_eq!(embedded[0], first[0]);
        app.doc.icons.clear();
        assert!(
            app.cloud_candidates(Some(CloudProvider::Aws), "vm")
                .is_empty()
        );
    }

    #[test]
    fn favorites_and_recents_use_exact_versions_and_embedded_fallback() {
        let old = pack(CloudProvider::Aws, "v1").icons.remove(0);
        let current = pack(CloudProvider::Aws, "v2");
        let current_ref = current.icons[0].reference.clone();
        let azure = pack(CloudProvider::Azure, "v1");
        let azure_ref = azure.icons[0].reference.clone();
        let mut app = BlueprintApp::new_with_workspace(
            &egui::Context::default(),
            None,
            crate::preferences::WorkspacePreferences {
                favorites: vec![old.reference.clone(), azure_ref.clone()],
                ..Default::default()
            },
            vec![current, azure],
        );
        let (available, missing) = app.cloud_saved_icons(&app.preferences.favorites);
        assert!(
            available.is_empty(),
            "a newer pack must not replace a favorite's artwork"
        );
        assert_eq!(missing.as_slice(), std::slice::from_ref(&old.reference));
        app.doc.icons.insert(old.reference.clone(), old.clone());
        let (available, missing) = app.cloud_saved_icons(&app.preferences.favorites);
        assert_eq!(available.as_slice(), std::slice::from_ref(&old));
        assert!(missing.is_empty());
        app.palette.note_used(&old.reference);
        app.palette.note_used(&current_ref);
        let (recent, _) = app.cloud_saved_icons(&app.palette.recent);
        assert_eq!(
            recent
                .iter()
                .map(|icon| &icon.reference)
                .collect::<Vec<_>>(),
            [&current_ref, &old.reference]
        );
        app.cloud.provider = CloudProvider::Azure;
        assert_eq!(
            app.cloud_saved_icons(&app.preferences.favorites).0[0].reference,
            azure_ref
        );
        app.cloud.packs.clear();
        let (available, missing) = app.cloud_saved_icons(&app.preferences.favorites);
        assert!(available.is_empty());
        assert_eq!(missing.as_slice(), std::slice::from_ref(&azure_ref));
        app.cloud.provider = CloudProvider::Aws;
        assert_eq!(app.cloud_saved_icons(&app.preferences.favorites).0[0], old);
    }
}
