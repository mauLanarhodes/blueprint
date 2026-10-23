//! Explicit resource settings and an asynchronous preview → deploy workflow.

use crate::BlueprintApp;
use bp_commands::{Command, PageProp, Prop};
use bp_deploy::{Plan, Terraform};
use bp_model::kurbo::{Point, Rect};
use bp_model::{
    AwsResource, AwsResourceKind, DiagramKind, Element, ElementId, NetworkContainer, OrderKey,
    Parent, ShapeRef,
};
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
    Planned(Result<Box<Plan>, String>),
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
        let mut vpc = network_element(
            NetworkContainer::Vpc,
            parent,
            order,
            Rect::from_center_size(center, (1000.0, 560.0)),
        );
        let data = vpc.as_shape_mut().unwrap();
        data.text = "Private VPC".into();
        data.infrastructure = Some(AwsResource::Vpc {
            cidr: "10.0.0.0/16".into(),
        });
        let mut commands = Vec::new();
        let mut selection = vec![vpc.id];
        let mut order = OrderKey::first();
        for (index, x) in [-468.0, 16.0].into_iter().enumerate() {
            let mut subnet = network_element(
                NetworkContainer::Subnet,
                Parent::Element(vpc.id),
                order.clone(),
                Rect::new(
                    center.x + x,
                    center.y - 190.0,
                    center.x + x + 452.0,
                    center.y + 230.0,
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
            selection.push(subnet.id);
            commands.push(Command::Insert(Box::new(subnet)));
        }
        commands.insert(0, Command::Insert(Box::new(vpc)));
        crate::presets::commands(&mut commands, self.doc.pages[&self.page].background);
        if self.apply("Add deployable AWS network", commands) {
            self.selection = selection;
            self.fit_requested = true;
            self.status =
                "Starter network added. Drag services into a subnet to organize the diagram."
                    .into();
        }
    }

    pub fn add_vpc(&mut self) {
        if !self.can_add_network_container() {
            return;
        }
        let parent = Parent::Layer(self.layer);
        let mut element = network_element(
            NetworkContainer::Vpc,
            parent,
            self.doc.next_order_key(parent),
            Rect::from_center_size(self.network_view_center(), (1000.0, 560.0)),
        );
        element.as_shape_mut().unwrap().text = "VPC".into();
        element.as_shape_mut().unwrap().infrastructure =
            Some(AwsResourceKind::Vpc.create(element.id));
        let id = element.id;
        let mut commands = vec![Command::Insert(Box::new(element))];
        crate::presets::commands(&mut commands, self.doc.pages[&self.page].background);
        if self.apply("Add VPC", commands) {
            self.selection = vec![id];
            self.status = "VPC added. Edit its CIDR in Infrastructure settings.".into();
        }
    }

    pub fn add_subnet(&mut self) {
        if self.page_kind() != Some(DiagramKind::Cloud) || self.deployment.busy() {
            return;
        }
        let selected_vpc = self.selection.iter().copied().find(|id| {
            self.doc.elements.get(id).is_some_and(|element| {
                element.network_container() == Some(NetworkContainer::Vpc)
                    && self.doc.page_of(*id) == Some(self.page)
                    && !self.doc.is_locked(*id)
                    && self
                        .doc
                        .layer_of(*id)
                        .is_some_and(|layer| self.doc.layers[&layer].visible)
                    && matches!(
                        element
                            .as_shape()
                            .and_then(|shape| shape.infrastructure.as_ref()),
                        Some(AwsResource::Vpc { .. })
                    )
            })
        });
        if selected_vpc.is_none() && !self.can_add_network_container() {
            return;
        }
        let (parent, bounds) = if let Some(vpc) = selected_vpc {
            let Some(bounds) = vacant_subnet_bounds(&self.doc, vpc) else {
                self.status =
                    "There is no room for another subnet. Resize the selected VPC first.".into();
                return;
            };
            (Parent::Element(vpc), bounds)
        } else {
            (
                Parent::Layer(self.layer),
                Rect::from_center_size(self.network_view_center(), (400.0, 300.0)),
            )
        };
        let mut element = network_element(
            NetworkContainer::Subnet,
            parent,
            self.doc.next_order_key(parent),
            bounds,
        );
        let data = element.as_shape_mut().unwrap();
        data.text = "Private subnet".into();
        data.infrastructure = Some(AwsResource::Subnet {
            vpc: selected_vpc,
            // CIDR allocation is a network decision. Require an explicit value.
            cidr: String::new(),
            availability_zone: String::new(),
        });
        let id = element.id;
        let mut commands = vec![Command::Insert(Box::new(element))];
        crate::presets::commands(&mut commands, self.doc.pages[&self.page].background);
        if self.apply("Add subnet", commands) {
            self.selection = vec![id];
            self.status = "Subnet added. Choose its IPv4 CIDR before deploying.".into();
        }
    }

    pub fn convert_network_to_containers(&mut self) {
        if self.page_kind() != Some(DiagramKind::Cloud) || self.deployment.busy() {
            return;
        }
        match bp_commands::network::convert(&self.doc, self.page) {
            Ok(commands) if commands.is_empty() => {
                self.status = "This page's network resources are already containers.".into();
            }
            Ok(commands) => {
                if self.apply("Convert network to containers", commands) {
                    self.fit_requested = true;
                    self.status = "Network converted. Resource identities and authored connectors are preserved.".into();
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn can_add_network_container(&mut self) -> bool {
        if self.page_kind() != Some(DiagramKind::Cloud) || self.deployment.busy() {
            return false;
        }
        if self
            .doc
            .layers
            .get(&self.layer)
            .is_none_or(|layer| layer.locked || !layer.visible)
        {
            self.status = "Choose a visible, unlocked layer for the network container".into();
            return false;
        }
        true
    }

    fn network_view_center(&self) -> Point {
        let center = self.view_center();
        if center.x.is_finite() && center.y.is_finite() {
            center
        } else {
            Point::ZERO
        }
    }

    pub fn add_aws_trial(&mut self) {
        self.add_trial_stack(false);
    }

    pub fn add_lambda_trial(&mut self) {
        self.add_trial_stack(true);
    }

    fn add_trial_stack(&mut self, with_lambda: bool) {
        if self.page_kind() != Some(DiagramKind::Cloud) || self.deployment.busy() {
            return;
        }
        let Some(layer) = self.doc.layers.get(&self.layer) else {
            return;
        };
        if layer.locked || !layer.visible {
            self.status = "Choose a visible, unlocked layer for the AWS trial".into();
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
        let elements = if with_lambda {
            bp_deploy::trial::elements_with_lambda(parent, center, order)
        } else {
            bp_deploy::trial::elements(parent, center, order)
        };
        let selection = elements
            .iter()
            .filter(|e| e.is_shape())
            .map(|e| e.id)
            .collect();
        let mut commands: Vec<_> = elements
            .into_iter()
            .map(|e| Command::Insert(Box::new(e)))
            .collect();
        crate::presets::commands(&mut commands, self.doc.pages[&self.page].background);
        if self.apply("Add AWS trial stack", commands) {
            self.selection = selection;
            self.fit_requested = true;
            self.status =
                "AWS trial added. Select a service to configure it, then preview deployment."
                    .into();
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
        let current = resource.as_ref().map(AwsResource::kind);
        let mut chosen = current;
        let resource_label = ui.label("Resource type");
        egui::ComboBox::from_id_salt(("resource-kind", element.id))
            .selected_text(resource.as_ref().map_or("Diagram only", AwsResource::label))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, "Diagram only");
                for kind in AwsResourceKind::ALL {
                    if matches!(shape.network_container, Some(NetworkContainer::Vpc))
                        && kind != AwsResourceKind::Vpc
                        || matches!(shape.network_container, Some(NetworkContainer::Subnet))
                            && kind != AwsResourceKind::Subnet
                    {
                        continue;
                    }
                    ui.selectable_value(&mut chosen, Some(kind), kind.label());
                }
            })
            .response
            .labelled_by(resource_label.id);
        if chosen != current {
            resource = chosen.map(|kind| kind.create(element.id));
            if let Some(AwsResource::Subnet { vpc, .. }) = &mut resource
                && shape.network_container == Some(NetworkContainer::Subnet)
                && let Parent::Element(parent) = element.parent
                && self.doc.elements[&parent].network_container() == Some(NetworkContainer::Vpc)
            {
                *vpc = Some(parent);
            }
        }
        if resource.is_none()
            && let Some(icon) = self.cloud_icon(&shape.shape)
            && let Some(kind) = AwsResourceKind::suggest(&icon.name)
            && ui
                .button(format!("Configure as {}", kind.label()))
                .clicked()
        {
            resource = Some(kind.create(element.id));
        }
        match &mut resource {
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
                        !self.doc.is_locked(e.id)
                            && self
                                .doc
                                .layer_of(e.id)
                                .is_some_and(|layer| self.doc.layers[&layer].visible)
                            && matches!(
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
                let vpc_label = ui.label("VPC");
                egui::ComboBox::from_id_salt(("subnet-vpc", element.id))
                    .selected_text(
                        candidates
                            .iter()
                            .find(|(id, _)| Some(*id) == *vpc)
                            .map_or("Choose VPC…", |(_, name)| name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        // An assigned subnet cannot be detached by clearing its
                        // deployment reference. The shared planner owns transfers.
                        if vpc.is_none() {
                            ui.selectable_value(vpc, None, "Choose VPC…");
                        }
                        for (id, name) in candidates {
                            ui.selectable_value(vpc, Some(id), name);
                        }
                    })
                    .response
                    .labelled_by(vpc_label.id);
                field(ui, "IPv4 CIDR", cidr);
                field(ui, "Availability zone (optional)", availability_zone);
                ui.label(
                    RichText::new("Private subnet: no public IP assignment or internet gateway.")
                        .small()
                        .weak(),
                );
            }
            Some(AwsResource::S3Bucket { name, versioning }) => {
                field(ui, "Bucket name (globally unique)", name);
                ui.checkbox(versioning, "Enable object versioning");
                ui.label(RichText::new("Public access is blocked. Bucket deletion will fail while it contains objects; objects are never force-deleted.").small().weak());
            }
            Some(AwsResource::DynamoDbTable {
                name,
                partition_key,
                sort_key,
            }) => {
                field(ui, "Table name", name);
                field(ui, "Partition key", partition_key);
                field(ui, "Sort key (optional)", sort_key);
                ui.label(RichText::new("On-demand billing. Both keys use the String type; other item attributes need no schema.").small().weak());
            }
            Some(AwsResource::SnsTopic { name }) => {
                field(ui, "Topic name", name);
                ui.label(RichText::new("Standard topic. Select this topic in an SQS queue's settings to subscribe that queue.").small().weak());
            }
            Some(AwsResource::SqsQueue {
                name,
                visibility_timeout,
                retention_seconds,
                topic,
            }) => {
                field(ui, "Queue name", name);
                ui.horizontal(|ui| {
                    ui.label("Visibility timeout (seconds)");
                    ui.add(egui::DragValue::new(visibility_timeout).range(0..=43_200));
                });
                ui.horizontal(|ui| {
                    ui.label("Message retention (seconds)");
                    ui.add(egui::DragValue::new(retention_seconds).range(60..=1_209_600));
                });
                let candidates: Vec<_> = self
                    .doc
                    .page_elements(self.page)
                    .into_iter()
                    .filter(|e| {
                        matches!(
                            e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                            Some(AwsResource::SnsTopic { .. })
                        )
                    })
                    .map(|e| {
                        (
                            e.id,
                            e.text()
                                .filter(|s| !s.is_empty())
                                .unwrap_or("SNS topic")
                                .to_owned(),
                        )
                    })
                    .collect();
                ui.label("Subscribe to SNS topic (optional)");
                let selected = if topic.is_some() {
                    "Missing topic — choose again"
                } else {
                    "No subscription"
                };
                egui::ComboBox::from_id_salt(("queue-topic", element.id))
                    .selected_text(
                        candidates
                            .iter()
                            .find(|(id, _)| Some(*id) == *topic)
                            .map_or(selected, |(_, name)| name),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(topic, None, "No subscription");
                        for (id, name) in candidates {
                            ui.selectable_value(topic, Some(id), name);
                        }
                    });
                ui.label(RichText::new("Standard encrypted queue. A subscription also creates a queue policy allowing only the selected SNS topic. Drawing a connector alone does not create a subscription.").small().weak());
            }
            Some(AwsResource::LambdaFunction {
                name,
                source_code,
                timeout_seconds,
                memory_mb,
                queue,
            }) => {
                field(ui, "Function name", name);
                ui.label("Python 3.13 · lambda_function.lambda_handler");
                ui.horizontal(|ui| {
                    ui.label("Function timeout (seconds)");
                    ui.add(egui::DragValue::new(timeout_seconds).range(1..=900));
                });
                ui.horizontal(|ui| {
                    ui.label("Memory (MB)");
                    ui.add(egui::DragValue::new(memory_mb).range(128..=10_240));
                });
                let candidates: Vec<_> = self
                    .doc
                    .page_elements(self.page)
                    .into_iter()
                    .filter(|e| {
                        matches!(
                            e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                            Some(AwsResource::SqsQueue { .. })
                        )
                    })
                    .map(|e| {
                        (
                            e.id,
                            e.text()
                                .filter(|s| !s.is_empty())
                                .unwrap_or("SQS queue")
                                .to_owned(),
                        )
                    })
                    .collect();
                let selected = if queue.is_some() {
                    "Missing queue — choose again"
                } else {
                    "No SQS trigger"
                };
                let label = ui.label("SQS trigger");
                egui::ComboBox::from_id_salt(("lambda-queue", element.id))
                    .selected_text(
                        candidates
                            .iter()
                            .find(|(id, _)| Some(*id) == *queue)
                            .map_or(selected, |(_, name)| name),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(queue, None, "No SQS trigger");
                        for (id, name) in candidates {
                            ui.selectable_value(queue, Some(id), name);
                        }
                    })
                    .response
                    .labelled_by(label.id);
                let label = ui.label("Python source (lambda_function.py)");
                ui.add(
                    egui::TextEdit::multiline(source_code)
                        .code_editor()
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
                ui.label(RichText::new("One Python module, up to 64 KiB, stored in this diagram. Define lambda_handler(event, context). The SQS queue needs at least six times this function's timeout.").small().weak());
            }
            None => {
                ui.label(
                    RichText::new("Choose a resource type to include this shape in a deployment.")
                        .small()
                        .weak(),
                );
            }
        }
        if shape.network_container == Some(NetworkContainer::Subnet)
            || self.doc.ancestors(element.id).iter().any(|id| {
                self.doc.elements[id].network_container() == Some(NetworkContainer::Subnet)
            })
        {
            ui.label(RichText::new("Subnet placement organizes this diagram. It does not configure service networking or permissions.").small().weak());
        }
        if resource != shape.infrastructure
            && let Err(error) = self.set_resource_configuration(element.id, resource)
        {
            self.status = error;
        }
    }

    /// Shared with the inspector tests: relationship and deployment changes
    /// must be accepted or rejected together, including deterministic placement.
    fn set_resource_configuration(
        &mut self,
        id: ElementId,
        resource: Option<AwsResource>,
    ) -> Result<(), String> {
        let element = self
            .doc
            .elements
            .get(&id)
            .ok_or("Resource no longer exists")?;
        let shape = element.as_shape().ok_or("Select a resource shape")?;
        let original_vpc = match shape.infrastructure.as_ref() {
            Some(AwsResource::Subnet { vpc, .. }) => *vpc,
            _ => None,
        };
        let new_vpc = match resource.as_ref() {
            Some(AwsResource::Subnet { vpc, .. }) => *vpc,
            _ => None,
        };
        let mut commands = vec![Command::Set {
            id,
            prop: Prop::Infrastructure(resource.clone()),
        }];
        if let Some(vpc) = new_vpc
            && (original_vpc != Some(vpc)
                && !(shape.infrastructure.is_none() && element.parent == Parent::Element(vpc))
                || element.network_container() == Some(NetworkContainer::Subnet)
                    && element.parent != Parent::Element(vpc))
        {
            if element.network_container() != Some(NetworkContainer::Subnet)
                || self
                    .doc
                    .elements
                    .get(&vpc)
                    .and_then(Element::network_container)
                    != Some(NetworkContainer::Vpc)
            {
                return Err(
                    "Choose Convert network to containers before changing this subnet's VPC."
                        .into(),
                );
            }
            let mut staged = self.doc.clone();
            staged
                .elements
                .get_mut(&id)
                .unwrap()
                .as_shape_mut()
                .unwrap()
                .infrastructure = resource;
            commands.extend(
                bp_commands::network::reassign_subnet(&staged, id, vpc)
                    .map_err(|error| error.to_string())?,
            );
            if !self.apply("Reassign subnet VPC", commands) {
                return Err(self.status.clone());
            }
        } else {
            let mut staged = self.doc.clone();
            staged
                .elements
                .get_mut(&id)
                .unwrap()
                .as_shape_mut()
                .unwrap()
                .infrastructure = resource;
            staged.validate().map_err(|error| error.to_string())?;
            self.apply_merging(
                "Configure infrastructure",
                format!("infrastructure:{id}"),
                commands,
            );
        }
        self.invalidate_changed_deployment_plan();
        Ok(())
    }

    pub(crate) fn invalidate_changed_deployment_plan(&mut self) {
        if self.deployment.plan.as_ref().is_some_and(|plan| {
            !bp_deploy::configuration(&self.doc, self.page)
                .is_ok_and(|configuration| configuration.matches(&plan.configuration))
        }) {
            self.deployment.plan = None;
            self.deployment.reviewed = false;
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
                            self.deployment.plan = Some(*plan);
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
            let _ = sender.send(DeploymentEvent::Planned(result.map(Box::new)));
            ctx.request_repaint();
        });
    }

    fn apply_deployment(&mut self, ctx: &egui::Context) {
        if self.deployment.busy() || !self.deployment.reviewed {
            return;
        }
        let valid = self.deployment.plan.as_ref().is_some_and(|plan| {
            !plan.changes.is_empty()
                && bp_deploy::configuration(&self.doc, self.page)
                    .is_ok_and(|config| config.matches(&plan.configuration))
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
            !configuration
                .as_ref()
                .is_ok_and(|config| config.matches(&plan.configuration))
        }) {
            self.deployment.plan = None;
            self.deployment.reviewed = false;
        }
        let mut preview = false;
        let mut deploy = false;
        let mut starter = false;
        let mut convert = false;
        let mut trial = false;
        let mut lambda_trial = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("deploy-infrastructure")).show(ctx, |ui| {
            ui.set_width(680.0_f32.min((ctx.content_rect().width() - 48.0).max(240.0)));
            ui.heading("Deploy AWS infrastructure");
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 100.0).clamp(180.0, 640.0)).show(ui, |ui| {
                ui.label("Configure S3, DynamoDB, SNS, SQS, Lambda, VPCs or private subnets, preview the changes, then deploy the reviewed plan.");
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
                    convert = ui.button("Convert network to containers").clicked();
                    trial = ui.button("Add AWS trial stack").clicked();
                    lambda_trial = ui.button("Add Lambda trial").clicked();
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
        if convert {
            self.convert_network_to_containers();
        }
        if trial {
            self.add_aws_trial();
        }
        if lambda_trial {
            self.add_lambda_trial();
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

fn network_element(
    kind: NetworkContainer,
    parent: Parent,
    order: OrderKey,
    bounds: Rect,
) -> Element {
    let mut element = Element::shape(
        ShapeRef::new("basic", "rounded-rectangle"),
        parent,
        order,
        bounds,
    );
    element.as_shape_mut().unwrap().network_container = Some(kind);
    element
}

/// Reuse the transfer planner's placement search without assigning a CIDR.
fn vacant_subnet_bounds(doc: &bp_model::Document, vpc: ElementId) -> Option<Rect> {
    let content = NetworkContainer::Vpc.content_rect(doc.elements.get(&vpc)?.as_shape()?.bounds);
    let element = network_element(
        NetworkContainer::Subnet,
        Parent::Layer(doc.layer_of(vpc)?),
        OrderKey::first(),
        Rect::new(
            content.x0,
            content.y0,
            content.x0 + 400.0,
            content.y0 + 300.0,
        ),
    );
    let id = element.id;
    let mut staged = doc.clone();
    staged.elements.insert(id, element);
    let position = bp_commands::network::available_position(&staged, id, vpc).ok()?;
    Some(Rect::from_origin_size(position, (400.0, 300.0)))
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
        assert_eq!(app.doc.elements.len(), 3);
        let (vpc, subnets) = resource_ids(&app);
        assert_eq!(
            app.doc.elements[&vpc].network_container(),
            Some(NetworkContainer::Vpc)
        );
        let content =
            NetworkContainer::Vpc.content_rect(app.doc.elements[&vpc].as_shape().unwrap().bounds);
        for subnet in subnets {
            let element = &app.doc.elements[&subnet];
            assert_eq!(element.network_container(), Some(NetworkContainer::Subnet));
            assert_eq!(element.parent, Parent::Element(vpc));
            assert!(content.contains_rect(element.as_shape().unwrap().bounds));
        }
        assert_eq!(app.selection.len(), 3);
        app.undo();
        assert!(app.doc.elements.is_empty());
        app.redo();
        app.doc.validate().unwrap();
        assert_eq!(app.doc.elements.len(), 3);
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
    fn add_subnet_uses_selected_vpc_and_requires_an_explicit_cidr() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Cloud);
        app.add_vpc();
        let vpc = app.selection[0];
        app.add_subnet();
        let subnet = app.selection[0];
        assert_eq!(app.doc.elements[&subnet].parent, Parent::Element(vpc));
        assert!(
            matches!(app.doc.elements[&subnet].as_shape().unwrap().infrastructure.as_ref(),
            Some(AwsResource::Subnet { vpc: Some(parent), cidr, .. }) if *parent == vpc && cidr.is_empty())
        );
        let first_bounds = app.doc.elements[&subnet].as_shape().unwrap().bounds;
        app.selection = vec![vpc];
        app.add_subnet();
        let second = app.selection[0];
        assert_ne!(second, subnet);
        assert_eq!(
            first_bounds
                .intersect(app.doc.elements[&second].as_shape().unwrap().bounds)
                .area(),
            0.0
        );
        app.undo();
        assert!(!app.doc.elements.contains_key(&second));
        app.doc.validate().unwrap();
    }

    #[test]
    fn add_subnet_uses_selected_vpc_layer_even_when_active_layer_is_locked() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Cloud);
        app.add_vpc();
        let vpc = app.selection[0];
        let mut layer = bp_model::Layer::new(
            app.page,
            "Locked unrelated layer",
            OrderKey::after(&app.doc.layers[&app.layer].order),
        );
        layer.locked = true;
        app.layer = layer.id;
        assert!(app.apply("Add layer", [Command::InsertLayer(Box::new(layer))]));
        app.add_subnet();
        let subnet = app.selection[0];
        assert_ne!(subnet, vpc);
        assert_eq!(app.doc.elements[&subnet].parent, Parent::Element(vpc));
        assert!(!app.doc.is_locked(subnet));
        app.doc.validate().unwrap();
    }

    #[test]
    fn layout_and_service_membership_leave_terraform_identical() {
        let mut app = network_app();
        let (vpc, subnets) = resource_ids(&app);
        let service_bounds = NetworkContainer::Subnet
            .content_rect(app.doc.elements[&subnets[0]].as_shape().unwrap().bounds);
        let mut bucket = Element::shape(
            ShapeRef::new("basic", "rectangle"),
            Parent::Layer(app.layer),
            app.doc.next_order_key(Parent::Layer(app.layer)),
            Rect::new(
                service_bounds.x0,
                service_bounds.y0,
                service_bounds.x0 + 100.0,
                service_bounds.y0 + 70.0,
            ),
        );
        bucket.as_shape_mut().unwrap().text = "Storage".into();
        bucket.as_shape_mut().unwrap().infrastructure =
            Some(AwsResourceKind::S3Bucket.create(bucket.id));
        let bucket_id = bucket.id;
        assert!(app.apply("Add service", [Command::Insert(Box::new(bucket))]));
        let original = bp_deploy::configuration(&app.doc, app.page).unwrap();
        let commands = bp_commands::network::drop(
            &app.doc,
            &[bucket_id],
            Some(subnets[0]),
            Default::default(),
        )
        .unwrap();
        assert!(app.apply("Place service", commands));
        assert!(original.matches(&bp_deploy::configuration(&app.doc, app.page).unwrap()));
        let commands = bp_commands::edit::translate(
            &app.doc,
            &[vpc, subnets[0], bucket_id],
            (40.0, 25.0).into(),
        );
        assert!(app.apply("Move VPC", commands));
        let bounds = app.doc.elements[&vpc].as_shape().unwrap().bounds;
        let commands = bp_commands::network::resize(
            &app.doc,
            vpc,
            Rect::new(bounds.x0, bounds.y0, bounds.x1 + 100.0, bounds.y1 + 80.0),
        )
        .unwrap();
        assert!(app.apply("Resize VPC", commands));
        assert!(original.matches(&bp_deploy::configuration(&app.doc, app.page).unwrap()));
        let bucket_body: serde_json::Value = serde_json::from_str(&original.json).unwrap();
        assert!(
            bucket_body["resource"]["aws_s3_bucket"]
                .as_object()
                .unwrap()
                .values()
                .all(|body| body.get("subnet_id").is_none() && body.get("vpc_id").is_none())
        );
    }

    #[test]
    fn inspector_reassignment_updates_parent_and_dependency_in_one_undo_step() {
        let mut app = network_app();
        let (old_vpc, subnets) = resource_ids(&app);
        let original = bp_deploy::configuration(&app.doc, app.page).unwrap();
        app.add_vpc();
        let target = app.selection[0];
        let bounds = app.doc.elements[&target].as_shape().unwrap().bounds;
        assert!(app.apply(
            "Separate VPC",
            bp_commands::edit::translate(&app.doc, &[target], (bounds.width() + 80.0, 0.0).into())
        ));
        let before = serde_json::to_value(&app.doc).unwrap();
        let mut resource = app.doc.elements[&subnets[0]]
            .as_shape()
            .unwrap()
            .infrastructure
            .clone();
        if let Some(AwsResource::Subnet {
            vpc,
            availability_zone,
            ..
        }) = &mut resource
        {
            *vpc = Some(target);
            *availability_zone = "us-east-1a".into();
        }
        app.set_resource_configuration(subnets[0], resource)
            .unwrap();
        assert_eq!(
            app.doc.elements[&subnets[0]].parent,
            Parent::Element(target)
        );
        assert!(
            matches!(app.doc.elements[&subnets[0]].as_shape().unwrap().infrastructure, Some(AwsResource::Subnet { vpc: Some(id), .. }) if id == target)
        );
        let after = serde_json::to_value(&app.doc).unwrap();
        let configuration = bp_deploy::configuration(&app.doc, app.page).unwrap();
        assert!(!original.matches(&configuration));
        let generated: serde_json::Value = serde_json::from_str(&configuration.json).unwrap();
        let key = format!("r_{}", subnets[0].to_string().replace('-', ""));
        let dependency = generated["resource"]["aws_subnet"][&key]["vpc_id"]
            .as_str()
            .unwrap();
        assert!(dependency.contains(&target.to_string().replace('-', "")));
        assert!(!dependency.contains(&old_vpc.to_string().replace('-', "")));
        app.undo();
        assert_eq!(serde_json::to_value(&app.doc).unwrap(), before);
        app.redo();
        assert_eq!(serde_json::to_value(&app.doc).unwrap(), after);
        app.doc.validate().unwrap();
    }

    #[test]
    fn invalid_inspector_reassignment_keeps_all_settings_and_geometry() {
        let mut app = network_app();
        let (_, subnets) = resource_ids(&app);
        app.add_vpc();
        let target = app.selection[0];
        app.doc
            .elements
            .get_mut(&target)
            .unwrap()
            .as_shape_mut()
            .unwrap()
            .infrastructure = Some(AwsResource::Vpc {
            cidr: "192.168.0.0/16".into(),
        });
        let before = serde_json::to_value(&app.doc).unwrap();
        let mut resource = app.doc.elements[&subnets[0]]
            .as_shape()
            .unwrap()
            .infrastructure
            .clone();
        if let Some(AwsResource::Subnet { vpc, .. }) = &mut resource {
            *vpc = Some(target);
        }
        assert!(
            app.set_resource_configuration(subnets[0], resource)
                .unwrap_err()
                .contains("outside")
        );
        assert_eq!(serde_json::to_value(&app.doc).unwrap(), before);
    }

    #[test]
    fn diagram_only_subnet_can_be_configured_in_its_existing_visual_vpc() {
        let mut app = network_app();
        let (vpc, subnets) = resource_ids(&app);
        app.set_resource_configuration(subnets[0], None).unwrap();
        let resource = AwsResource::Subnet {
            vpc: Some(vpc),
            cidr: "10.0.3.0/24".into(),
            availability_zone: String::new(),
        };
        app.set_resource_configuration(subnets[0], Some(resource))
            .unwrap();
        assert_eq!(app.doc.elements[&subnets[0]].parent, Parent::Element(vpc));
        app.doc.validate().unwrap();
        bp_deploy::configuration(&app.doc, app.page).unwrap();
    }

    #[test]
    fn conversion_preserves_ids_terraform_and_authored_connectors_and_is_undoable() {
        let mut app = network_app();
        let (vpc, subnets) = resource_ids(&app);
        // Reconstruct an older drawing: configured resources on a layer.
        for id in [vpc, subnets[0], subnets[1]] {
            let element = app.doc.elements.get_mut(&id).unwrap();
            element.parent = Parent::Layer(app.layer);
            element.as_shape_mut().unwrap().network_container = None;
        }
        let connector = Element::connector(
            bp_model::Endpoint::glued(vpc, None),
            bp_model::Endpoint::glued(subnets[0], None),
            Parent::Layer(app.layer),
            app.doc.next_order_key(Parent::Layer(app.layer)),
        );
        let connector_id = connector.id;
        assert!(app.apply(
            "Authored relationship",
            [Command::Insert(Box::new(connector.clone()))]
        ));
        let before = serde_json::to_value(&app.doc).unwrap();
        let terraform = bp_deploy::configuration(&app.doc, app.page).unwrap();
        app.convert_network_to_containers();
        assert_eq!(app.doc.elements[&connector_id], connector);
        assert_eq!(
            app.doc.elements[&vpc].network_container(),
            Some(NetworkContainer::Vpc)
        );
        for subnet in subnets {
            assert_eq!(app.doc.elements[&subnet].parent, Parent::Element(vpc));
        }
        assert!(terraform.matches(&bp_deploy::configuration(&app.doc, app.page).unwrap()));
        app.doc.validate().unwrap();
        app.undo();
        assert_eq!(serde_json::to_value(&app.doc).unwrap(), before);
        app.redo();
        assert!(terraform.matches(&bp_deploy::configuration(&app.doc, app.page).unwrap()));
    }

    #[test]
    fn nested_network_reopens_and_previous_format_keeps_its_layout() {
        let app = network_app();
        let (vpc, subnets) = resource_ids(&app);
        let configuration = bp_deploy::configuration(&app.doc, app.page).unwrap();
        let reopened = bp_io::from_bytes(&bp_io::to_zip_bytes(&app.doc).unwrap()).unwrap();
        assert_eq!(reopened.elements[&subnets[0]].parent, Parent::Element(vpc));
        assert!(configuration.matches(&bp_deploy::configuration(&reopened, app.page).unwrap()));

        let mut legacy = app.doc.clone();
        legacy.schema_version = 8;
        for id in [vpc, subnets[0], subnets[1]] {
            let element = legacy.elements.get_mut(&id).unwrap();
            element.parent = Parent::Layer(app.layer);
            element.as_shape_mut().unwrap().network_container = None;
        }
        let original_bounds = legacy.elements[&vpc].as_shape().unwrap().bounds;
        let reopened = bp_io::from_bytes(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(
            reopened.elements[&vpc].as_shape().unwrap().bounds,
            original_bounds
        );
        assert_eq!(reopened.elements[&vpc].network_container(), None);
        assert_eq!(
            reopened.elements[&subnets[0]].parent,
            Parent::Layer(app.layer)
        );
        assert!(configuration.matches(&bp_deploy::configuration(&reopened, app.page).unwrap()));
    }

    #[test]
    fn legacy_inspector_transfer_requests_explicit_conversion() {
        let mut app = network_app();
        let (_, subnets) = resource_ids(&app);
        app.add_vpc();
        let target = app.selection[0];
        let element = app.doc.elements.get_mut(&subnets[0]).unwrap();
        element.parent = Parent::Layer(app.layer);
        element.as_shape_mut().unwrap().network_container = None;
        let before = serde_json::to_value(&app.doc).unwrap();
        let mut resource = app.doc.elements[&subnets[0]]
            .as_shape()
            .unwrap()
            .infrastructure
            .clone();
        if let Some(AwsResource::Subnet { vpc, .. }) = &mut resource {
            *vpc = Some(target);
        }
        assert!(
            app.set_resource_configuration(subnets[0], resource)
                .unwrap_err()
                .contains("Convert network to containers")
        );
        assert_eq!(serde_json::to_value(&app.doc).unwrap(), before);
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
        assert_eq!(roots, vec![fresh_vpc]);
        let copied_subnets = app.doc.descendants(fresh_vpc);
        assert_eq!(copied_subnets.len(), 2);
        for id in copied_subnets {
            assert_eq!(app.doc.elements[&id].parent, Parent::Element(fresh_vpc));
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

    #[cfg(unix)]
    #[test]
    fn reviewed_mock_plan_survives_layout_edits_and_is_invalidated_by_subnet_transfer() {
        use std::os::unix::fs::PermissionsExt;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let scratch = Scratch(
            std::env::temp_dir().join(format!("blueprint-network-plan-{}", ElementId::new())),
        );
        std::fs::create_dir(&scratch.0).unwrap();
        let executable = scratch.0.join("terraform-mock");
        std::fs::write(&executable, r#"#!/bin/sh
case "$1" in
  init|validate) : ;;
  workspace) printf 'default\n' ;;
  plan) printf 'reviewed-plan' > blueprint.tfplan ;;
  show)
    if [ "$2" = '-json' ]; then
      printf '{"planned_values":{"outputs":{"aws_account_id":{"value":"123456789012"}}},"resource_changes":[{"address":"aws_subnet.test","change":{"actions":["create"]}}]}'
    else printf 'Plan: 1 to add.\n'; fi ;;
  *) exit 1 ;;
esac
"#).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut app = network_app();
        let (vpc, subnets) = resource_ids(&app);
        app.add_vpc();
        let destination = app.selection[0];
        let content = NetworkContainer::Subnet
            .content_rect(app.doc.elements[&subnets[0]].as_shape().unwrap().bounds);
        let service = Element::shape(
            ShapeRef::new("basic", "rectangle"),
            Parent::Layer(app.layer),
            app.doc.next_order_key(Parent::Layer(app.layer)),
            Rect::new(content.x0, content.y0, content.x0 + 90.0, content.y0 + 60.0),
        );
        let id = service.id;
        assert!(app.apply("Add service", [Command::Insert(Box::new(service))]));
        let configuration = bp_deploy::configuration(&app.doc, app.page).unwrap();
        let plan = Terraform { executable }
            .plan(configuration, &scratch.0, |_| {})
            .unwrap();
        app.deployment.plan = Some(plan);
        app.deployment.reviewed = true;
        assert!(
            app.apply(
                "Place service",
                bp_commands::network::drop(&app.doc, &[id], Some(subnets[0]), Default::default())
                    .unwrap()
            )
        );
        assert!(app.apply(
            "Move VPC",
            bp_commands::edit::translate(&app.doc, &[vpc], (20.0, 30.0).into())
        ));
        let bounds = app.doc.elements[&vpc].as_shape().unwrap().bounds;
        assert!(
            app.apply(
                "Resize VPC",
                bp_commands::network::resize(
                    &app.doc,
                    vpc,
                    Rect::new(bounds.x0, bounds.y0, bounds.x1 + 40.0, bounds.y1 + 40.0)
                )
                .unwrap()
            )
        );
        assert!(app.deployment.plan.is_some());
        assert!(app.deployment.reviewed);
        let mut resource = app.doc.elements[&subnets[0]]
            .as_shape()
            .unwrap()
            .infrastructure
            .clone();
        if let Some(AwsResource::Subnet { vpc, .. }) = &mut resource {
            *vpc = Some(destination);
        }
        app.set_resource_configuration(subnets[0], resource)
            .unwrap();
        assert!(app.deployment.plan.is_none());
        assert!(!app.deployment.reviewed);
        assert!(!app.deployment.busy());
    }

    #[test]
    fn lambda_trial_is_one_undo_step_and_copy_remaps_or_clears_its_queue() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Cloud);
        app.add_lambda_trial();
        assert_eq!(app.selection.len(), 5);
        assert_eq!(app.doc.elements.len(), 7);
        app.undo();
        assert!(app.doc.elements.is_empty());
        app.redo();
        app.selection = app
            .doc
            .elements
            .values()
            .filter(|e| e.is_shape())
            .map(|e| e.id)
            .collect();
        let clip = Clip::copy(&app.doc, &app.selection, |_, _| None);
        let (roots, commands) = clip.paste(&app.doc, Parent::Layer(app.layer), Default::default());
        assert!(app.apply("Copy Lambda trial", commands));
        let copied_queue = roots
            .iter()
            .copied()
            .find(|id| {
                matches!(
                    app.doc.elements[id]
                        .as_shape()
                        .and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::SqsQueue { .. })
                )
            })
            .unwrap();
        let function = roots
            .iter()
            .copied()
            .find(|id| {
                matches!(
                    app.doc.elements[id]
                        .as_shape()
                        .and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::LambdaFunction { .. })
                )
            })
            .unwrap();
        assert!(
            matches!(app.doc.elements[&function].as_shape().unwrap().infrastructure, Some(AwsResource::LambdaFunction { queue: Some(id), .. }) if id == copied_queue)
        );
        let clip = Clip::copy(&app.doc, &[function], |_, _| None);
        let (roots, commands) = clip.paste(&app.doc, Parent::Layer(app.layer), Default::default());
        assert!(app.apply("Copy Lambda alone", commands));
        assert!(matches!(
            app.doc.elements[&roots[0]]
                .as_shape()
                .unwrap()
                .infrastructure,
            Some(AwsResource::LambdaFunction { queue: None, .. })
        ));
        app.doc.validate().unwrap();
    }

    #[test]
    fn trial_is_undoable_and_copying_remaps_its_subscription() {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Cloud);
        app.add_aws_trial();
        assert_eq!(app.selection.len(), 4);
        assert_eq!(app.doc.elements.len(), 5);
        app.undo();
        assert!(app.doc.elements.is_empty());
        app.redo();
        let selection: Vec<_> = app
            .doc
            .elements
            .values()
            .filter(|e| e.is_shape())
            .map(|e| e.id)
            .collect();
        let topic = app
            .doc
            .elements
            .values()
            .find(|e| {
                matches!(
                    e.as_shape().and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::SnsTopic { .. })
                )
            })
            .unwrap()
            .id;
        let clip = Clip::copy(&app.doc, &selection, |_, _| None);
        let (roots, commands) = clip.paste(&app.doc, Parent::Layer(app.layer), Default::default());
        assert!(app.apply("Copy AWS trial", commands));
        let copied_topic = roots
            .iter()
            .copied()
            .find(|id| {
                matches!(
                    app.doc.elements[id]
                        .as_shape()
                        .and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::SnsTopic { .. })
                )
            })
            .unwrap();
        assert_ne!(topic, copied_topic);
        let queue = roots
            .iter()
            .copied()
            .find(|id| {
                matches!(
                    app.doc.elements[id]
                        .as_shape()
                        .and_then(|s| s.infrastructure.as_ref()),
                    Some(AwsResource::SqsQueue { .. })
                )
            })
            .unwrap();
        assert!(
            matches!(app.doc.elements[&queue].as_shape().unwrap().infrastructure, Some(AwsResource::SqsQueue { topic: Some(id), .. }) if id == copied_topic)
        );
        let clip = Clip::copy(&app.doc, &[queue], |_, _| None);
        let (roots, commands) = clip.paste(&app.doc, Parent::Layer(app.layer), Default::default());
        assert!(app.apply("Copy only queue", commands));
        assert!(matches!(
            app.doc.elements[&roots[0]]
                .as_shape()
                .unwrap()
                .infrastructure,
            Some(AwsResource::SqsQueue { topic: None, .. })
        ));
        app.doc.validate().unwrap();
    }
}
