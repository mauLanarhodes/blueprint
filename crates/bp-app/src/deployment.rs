//! Explicit resource settings and an asynchronous preview → deploy workflow.

use crate::BlueprintApp;
use bp_commands::{Command, PageProp, Prop};
use bp_deploy::{Plan, Terraform};
use bp_model::kurbo::Rect;
use bp_model::{AwsResource, DiagramKind, Element, Endpoint, OrderKey, Parent, ShapeRef};
use egui::{RichText, Ui};
use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Default)]
pub struct DeploymentState {
    pub open: bool,
    pub root: Option<PathBuf>,
    pub plan: Option<Plan>,
    pub message: Option<String>,
    pub phase: String,
    reviewed: bool,
    worker: Option<mpsc::Receiver<DeploymentEvent>>,
}

enum DeploymentEvent {
    Progress(String),
    Planned(Result<Plan, String>),
    Applied(Result<String, String>),
}

impl DeploymentState {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
}

impl BlueprintApp {
    pub fn open_deployment(&mut self) {
        if self.page_kind() != Some(DiagramKind::Cloud) {
            return;
        }
        self.finish_inline_edits();
        self.cancel_drag();
        self.quick_insert = None;
        self.deployment.open = true;
    }

    pub fn add_starter_network(&mut self) {
        if self.page_kind() != Some(DiagramKind::Cloud) || self.deployment.busy() {
            return;
        }
        let Some(layer) = self.doc.layers.get(&self.layer) else {
            return;
        };
        if layer.locked || !layer.visible {
            self.status = "Choose a visible, unlocked layer for the starter network".into();
            return;
        }
        let parent = Parent::Layer(self.layer);
        let center = self.view_center();
        let center = if center.x.is_finite() && center.y.is_finite() {
            center
        } else {
            bp_model::kurbo::Point::ZERO
        };
        let order = self.doc.next_order_key(parent);
        let mut vpc = Element::shape(
            ShapeRef::new("basic", "rounded-rectangle"),
            parent,
            order.clone(),
            Rect::new(
                center.x - 100.0,
                center.y - 140.0,
                center.x + 100.0,
                center.y - 60.0,
            ),
        );
        let data = vpc.as_shape_mut().unwrap();
        data.text = "Private VPC".into();
        data.infrastructure = Some(AwsResource::Vpc {
            cidr: "10.0.0.0/16".into(),
        });
        let mut commands = Vec::new();
        let mut selection = vec![vpc.id];
        let mut order = order;
        for (index, x) in [-210.0, 10.0].into_iter().enumerate() {
            order = OrderKey::after(&order);
            let mut subnet = Element::shape(
                ShapeRef::new("basic", "rounded-rectangle"),
                parent,
                order.clone(),
                Rect::new(
                    center.x + x,
                    center.y + 30.0,
                    center.x + x + 200.0,
                    center.y + 110.0,
                ),
            );
            let data = subnet.as_shape_mut().unwrap();
            data.text = format!("Private subnet {}", index + 1);
            data.infrastructure = Some(AwsResource::Subnet {
                vpc: Some(vpc.id),
                cidr: format!("10.0.{}.0/24", index + 1),
                availability_zone: String::new(),
            });
            order = OrderKey::after(&order);
            let connector = Element::connector(
                Endpoint::glued(vpc.id, None),
                Endpoint::glued(subnet.id, None),
                parent,
                order.clone(),
            );
            selection.push(subnet.id);
            commands.push(Command::Insert(Box::new(subnet)));
            commands.push(Command::Insert(Box::new(connector)));
        }
        commands.insert(0, Command::Insert(Box::new(vpc)));
        crate::presets::commands(&mut commands, self.doc.pages[&self.page].background);
        if self.apply("Add deployable AWS network", commands) {
            self.selection = selection;
            self.fit_requested = true;
            self.status =
                "Starter network added. Select a shape to edit its deployment settings.".into();
        }
    }

    pub(crate) fn resource_inspector(&mut self, ui: &mut Ui, element: &Element) {
        if self.page_kind() != Some(DiagramKind::Cloud) {
            return;
        }
        let Some(shape) = element.as_shape() else {
            return;
        };
        ui.separator();
        ui.label(RichText::new("Infrastructure").strong());
        if shape.shape.library() == "azure" {
            ui.label("Azure deployment is not available yet.");
            return;
        }
        let mut resource = shape.infrastructure.clone();
        let current = match resource {
            None => 0,
            Some(AwsResource::Vpc { .. }) => 1,
            Some(AwsResource::Subnet { .. }) => 2,
            Some(AwsResource::Resources { .. }) => 3,
        };
        let mut chosen = current;
        let resource_label = ui.label("Resource type");
        egui::ComboBox::from_id_salt(("resource-kind", element.id))
            .selected_text(resource.as_ref().map_or("Diagram only", AwsResource::label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, 0, "Diagram only");
                ui.selectable_value(&mut chosen, 1, "AWS VPC");
                ui.selectable_value(&mut chosen, 2, "AWS private subnet");
                if current == 3 {
                    ui.selectable_value(&mut chosen, 3, "AWS resource configuration");
                }
            })
            .response
            .labelled_by(resource_label.id);
        if chosen != current {
            resource = match chosen {
                1 => Some(AwsResource::Vpc {
                    cidr: "10.0.0.0/16".into(),
                }),
                2 => Some(AwsResource::Subnet {
                    vpc: None,
                    cidr: "10.0.1.0/24".into(),
                    availability_zone: String::new(),
                }),
                _ => None,
            };
        }
        match &mut resource {
            Some(AwsResource::Resources { resources }) => {
                ui.label(format!("{} saved resource draft(s)", resources.len()));
                ui.label("General resource editing and deployment will be available in the next checkpoint.");
            }
            Some(AwsResource::Vpc { cidr }) => {
                field(ui, "IPv4 CIDR", cidr);
            }
            Some(AwsResource::Subnet {
                vpc,
                cidr,
                availability_zone,
            }) => {
                let candidates: Vec<_> = self
                    .doc
                    .page_elements(self.page)
                    .into_iter()
                    .filter(|e| {
                        matches!(
                            e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                            Some(AwsResource::Vpc { .. })
                        )
                    })
                    .map(|e| {
                        (
                            e.id,
                            e.text()
                                .filter(|s| !s.is_empty())
                                .unwrap_or("VPC")
                                .to_owned(),
                        )
                    })
                    .collect();
                ui.label("VPC");
                egui::ComboBox::from_id_salt(("subnet-vpc", element.id))
                    .selected_text(
                        candidates
                            .iter()
                            .find(|(id, _)| Some(*id) == *vpc)
                            .map_or("Choose VPC…", |(_, name)| name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(vpc, None, "Choose VPC…");
                        for (id, name) in candidates {
                            ui.selectable_value(vpc, Some(id), name);
                        }
                    });
                field(ui, "IPv4 CIDR", cidr);
                field(ui, "Availability zone (optional)", availability_zone);
                ui.label(
                    RichText::new("Private subnet: no public IP assignment or internet gateway.")
                        .small()
                        .weak(),
                );
            }
            None => {
                ui.label(
                    RichText::new("Choose a resource type to include this shape in a deployment.")
                        .small()
                        .weak(),
                );
            }
        }
        if resource != shape.infrastructure {
            self.apply_merging(
                "Configure infrastructure",
                format!("infrastructure:{}", element.id),
                vec![Command::Set {
                    id: element.id,
                    prop: Prop::Infrastructure(resource),
                }],
            );
        }
    }

    pub(crate) fn deployment_target(&mut self, ui: &mut Ui) {
        let Some(page) = self.doc.pages.get(&self.page) else {
            return;
        };
        let original = page.deployment.clone();
        let mut target = original.clone().unwrap_or_default();
        let before = target.clone();
        field(ui, "AWS account ID", &mut target.account_id);
        field(ui, "AWS region", &mut target.region);
        field(ui, "AWS profile (optional)", &mut target.profile);
        ui.label(RichText::new("Uses your local AWS login or environment credentials. Credentials are never saved in the diagram.").small().weak());
        if target != before {
            self.apply_merging(
                "AWS deployment target",
                format!("deployment:{}", self.page),
                vec![Command::SetPage {
                    id: self.page,
                    prop: PageProp::Deployment(Some(target)),
                }],
            );
        }
    }

    pub(crate) fn poll_deployment(&mut self) {
        while let Some(receiver) = &self.deployment.worker {
            let event = match receiver.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.deployment.worker = None;
                    self.deployment.message = Some("Deployment stopped unexpectedly. Preview again to check the current infrastructure state.".into());
                    break;
                }
            };
            match event {
                DeploymentEvent::Progress(phase) => self.deployment.phase = phase,
                DeploymentEvent::Planned(result) => {
                    self.deployment.worker = None;
                    self.deployment.reviewed = false;
                    match result {
                        Ok(plan) => {
                            self.deployment.plan = Some(plan);
                            self.deployment.message = None;
                        }
                        Err(error) => self.deployment.message = Some(error),
                    }
                }
                DeploymentEvent::Applied(result) => {
                    self.deployment.worker = None;
                    self.deployment.reviewed = false;
                    self.deployment.message = Some(match result {
                        Ok(output) => {
                            self.status = "AWS deployment completed".into();
                            output
                        }
                        Err(error) => {
                            self.status =
                                "Deployment failed; review current AWS state before retrying"
                                    .into();
                            format!(
                                "{error}\nAn apply can partially succeed. Keep this workspace and preview again before retrying."
                            )
                        }
                    });
                }
            }
        }
    }

    pub fn preview_deployment(&mut self, ctx: &egui::Context) {
        if self.deployment.busy() {
            return;
        }
        let Some(root) = self.deployment.root.clone() else {
            self.deployment.message = Some("Choose a deployment folder first".into());
            return;
        };
        let configuration = match bp_deploy::configuration(&self.doc, self.page) {
            Ok(config) => config,
            Err(error) => {
                self.deployment.message = Some(error);
                return;
            }
        };
        self.deployment.plan = None;
        self.deployment.message = None;
        self.deployment.reviewed = false;
        let (sender, receiver) = mpsc::channel();
        self.deployment.worker = Some(receiver);
        self.deployment.phase = "Preparing deployment preview…".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = Terraform::default().plan(configuration, &root, |phase| {
                let _ = sender.send(DeploymentEvent::Progress(phase.into()));
                ctx.request_repaint();
            });
            let _ = sender.send(DeploymentEvent::Planned(result));
            ctx.request_repaint();
        });
    }

    fn apply_deployment(&mut self, ctx: &egui::Context) {
        if self.deployment.busy() || !self.deployment.reviewed {
            return;
        }
        let valid = self.deployment.plan.as_ref().is_some_and(|plan| {
            !plan.changes.is_empty()
                && bp_deploy::configuration(&self.doc, self.page).is_ok_and(|config| {
                    config.page == plan.configuration.page && config.json == plan.configuration.json
                })
        });
        if !valid {
            self.deployment.plan = None;
            self.deployment.message =
                Some("Resource settings changed. Preview again before deploying.".into());
            return;
        }
        let plan = self.deployment.plan.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        self.deployment.worker = Some(receiver);
        self.deployment.message = None;
        self.deployment.phase = "Starting AWS deployment…".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = Terraform::default().apply(plan, |phase| {
                let _ = sender.send(DeploymentEvent::Progress(phase.into()));
                ctx.request_repaint();
            });
            let _ = sender.send(DeploymentEvent::Applied(result));
            ctx.request_repaint();
        });
    }

    pub(crate) fn deployment_dialog(&mut self, ctx: &egui::Context) {
        if !self.deployment.open {
            return;
        }
        let busy = self.deployment.busy();
        let configuration = bp_deploy::configuration(&self.doc, self.page);
        if self.deployment.plan.as_ref().is_some_and(|plan| {
            !configuration.as_ref().is_ok_and(|config| {
                config.page == plan.configuration.page && config.json == plan.configuration.json
            })
        }) {
            self.deployment.plan = None;
            self.deployment.reviewed = false;
        }
        let mut preview = false;
        let mut deploy = false;
        let mut starter = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("deploy-infrastructure")).show(ctx, |ui| {
            ui.set_width(680.0_f32.min((ctx.content_rect().width() - 48.0).max(240.0)));
            ui.heading("Deploy AWS infrastructure");
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 100.0).clamp(180.0, 640.0)).show(ui, |ui| {
                ui.label("Configure a private VPC and subnets, preview the changes, then deploy the reviewed plan.");
                ui.add_enabled_ui(!busy, |ui| {
                    self.deployment_target(ui);
                    ui.horizontal(|ui| {
                        if ui.button("Choose deployment folder…").clicked()
                            && let Some(root) = rfd::FileDialog::new().pick_folder() {
                            self.deployment.root = Some(root);
                            self.deployment.plan = None;
                            self.deployment.reviewed = false;
                        }
                        if let Some(root) = &self.deployment.root { ui.label(root.display().to_string()); }
                    });
                    ui.label(RichText::new("Keep the deployment folder: it stores Terraform state for future updates. Each page gets its own subfolder.").small().weak());
                    starter = ui.button("Add starter network").clicked();
                });
                ui.separator();
                match &configuration {
                    Ok(config) => {
                        ui.label(RichText::new("Resources in this deployment").strong());
                        for resource in &config.resources { ui.label(resource); }
                        for warning in &config.warnings { ui.label(warning); }
                        egui::CollapsingHeader::new("Generated Terraform configuration").show(ui, |ui| {
                            ui.monospace(&config.json);
                        });
                    }
                    Err(error) => { ui.label(error); }
                }
                preview = ui.add_enabled(!busy && configuration.is_ok() && self.deployment.root.is_some(), egui::Button::new("Preview deployment")).clicked();
                if busy {
                    ui.horizontal(|ui| { ui.spinner(); ui.label(&self.deployment.phase); });
                }
                if let Some(plan) = &self.deployment.plan {
                    ui.separator();
                    ui.label(RichText::new("Reviewed Terraform plan").strong());
                    if plan.changes.is_empty() { ui.label("Infrastructure already matches this configuration."); }
                    for change in &plan.changes { ui.label(change); }
                    if plan.destroys_resources { ui.label(RichText::new("This plan deletes or replaces existing resources.").color(ui.visuals().error_fg_color)); }
                    ui.monospace(&plan.review);
                    if !plan.changes.is_empty() {
                        ui.label(format!("Deployment changes real resources in AWS account {} and may incur charges.", plan.configuration.account_id));
                        ui.checkbox(&mut self.deployment.reviewed, "I reviewed this plan and want to apply these changes");
                        deploy = ui.add_enabled(!busy && self.deployment.reviewed, egui::Button::new("Deploy reviewed plan")).clicked();
                    }
                }
                if let Some(message) = &self.deployment.message { ui.separator(); ui.monospace(message); }
                close = ui.add_enabled(!busy, egui::Button::new("Close")).clicked();
            });
        });
        if !busy && (close || modal.should_close()) {
            self.deployment.open = false;
        }
        if starter {
            self.add_starter_network();
        }
        if preview {
            self.preview_deployment(ctx);
        }
        if deploy {
            self.apply_deployment(ctx);
        }
    }
}

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    let label = ui.label(label);
    ui.add(crate::widgets::text_field(value).desired_width(f32::INFINITY))
        .labelled_by(label.id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_commands::edit::Clip;
    use bp_model::{AwsDeployment, ElementId};

    fn network_app() -> BlueprintApp {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Cloud);
        app.add_starter_network();
        app.apply(
            "Set AWS target",
            [Command::SetPage {
                id: app.page,
                prop: PageProp::Deployment(Some(AwsDeployment {
                    account_id: "123456789012".into(),
                    ..Default::default()
                })),
            }],
        );
        app
    }

    fn resource_ids(app: &BlueprintApp) -> (ElementId, Vec<ElementId>) {
        let elements = app.doc.page_elements(app.page);
        let vpc = elements
            .iter()
            .find(|e| {
                matches!(
                    e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::Vpc { .. })
                )
            })
            .unwrap()
            .id;
        let subnets = elements
            .iter()
            .filter(|e| {
                matches!(
                    e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::Subnet { .. })
                )
            })
            .map(|e| e.id)
            .collect();
        (vpc, subnets)
    }

    #[test]
    fn starter_is_one_undo_step_and_generates_three_resources() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Cloud);
        app.add_starter_network();
        assert_eq!(app.doc.elements.len(), 5);
        assert_eq!(app.selection.len(), 3);
        app.undo();
        assert!(app.doc.elements.is_empty());
        app.redo();
        app.doc.validate().unwrap();
        assert_eq!(app.doc.elements.len(), 5);
        let app = network_app();
        assert_eq!(
            bp_deploy::configuration(&app.doc, app.page)
                .unwrap()
                .resources
                .len(),
            3
        );
    }

    #[test]
    fn copied_networks_remap_vpcs_and_isolated_subnets_clear_the_dependency() {
        let mut app = network_app();
        let (vpc, subnets) = resource_ids(&app);
        let clip = Clip::copy(&app.doc, &app.selection, |_, _| None);
        let (roots, commands) = clip.paste(&app.doc, Parent::Layer(app.layer), Default::default());
        assert!(app.apply("Paste network", commands));
        let fresh_vpc = roots
            .iter()
            .find(|id| {
                matches!(
                    app.doc.elements[id]
                        .as_shape()
                        .and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::Vpc { .. })
                )
            })
            .copied()
            .unwrap();
        assert_ne!(fresh_vpc, vpc);
        for id in roots {
            if let Some(AwsResource::Subnet { vpc, .. }) = app.doc.elements[&id]
                .as_shape()
                .and_then(|s| s.infrastructure.as_ref())
            {
                assert_eq!(*vpc, Some(fresh_vpc));
            }
        }
        assert_eq!(
            bp_deploy::configuration(&app.doc, app.page)
                .unwrap()
                .resources
                .len(),
            6
        );
        let clip = Clip::copy(&app.doc, &[subnets[0]], |_, _| None);
        let (roots, commands) = clip.paste(&app.doc, Parent::Layer(app.layer), Default::default());
        assert!(app.apply("Paste subnet", commands));
        assert!(matches!(
            app.doc.elements[&roots[0]]
                .as_shape()
                .unwrap()
                .infrastructure,
            Some(AwsResource::Subnet { vpc: None, .. })
        ));
        assert!(bp_deploy::configuration(&app.doc, app.page).is_err());
    }

    #[test]
    fn duplicate_pages_keep_settings_and_have_independent_resource_identities() {
        let mut app = network_app();
        let source = app.page;
        let old_vpc = resource_ids(&app).0;
        let original = bp_deploy::configuration(&app.doc, source).unwrap();
        app.duplicate_page(source);
        assert_ne!(source, app.page);
        assert_eq!(
            app.doc.pages[&source].deployment,
            app.doc.pages[&app.page].deployment
        );
        let (vpc, subnets) = resource_ids(&app);
        assert_ne!(vpc, old_vpc);
        for id in subnets {
            assert!(
                matches!(app.doc.elements[&id].as_shape().unwrap().infrastructure, Some(AwsResource::Subnet { vpc: Some(parent), .. }) if parent == vpc)
            );
        }
        let copied = bp_deploy::configuration(&app.doc, app.page).unwrap();
        assert_ne!(original.json, copied.json);
        app.doc.validate().unwrap();
    }

    #[test]
    fn changed_resource_settings_prevent_deployment_even_after_review() {
        let mut app = network_app();
        app.deployment.reviewed = true;
        app.apply_deployment(&egui::Context::default());
        assert!(!app.deployment.busy());
        assert!(
            app.deployment
                .message
                .as_ref()
                .unwrap()
                .contains("Preview again")
        );
    }
}
