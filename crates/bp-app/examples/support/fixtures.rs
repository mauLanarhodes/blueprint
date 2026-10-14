//! Populated screenshot documents. All cloud artwork here is original test art;
//! it is never installed as a provider pack or substituted for vendor artwork.

use bp_app::BlueprintApp;
use bp_commands::{Command, LayerProp, PageProp, Prop};
use bp_icons::IconPack;
use bp_model::kurbo::Rect;
use bp_model::{
    CloudIcon, CloudProvider, Color, Dash, DiagramKind, ElementId, Endpoint, IconKind, Marker,
    Paint, Routing, ShapeRef, SqlDialect, TextAlign, VerticalAlign,
};

#[derive(Clone, Copy, Default)]
pub struct Targets {
    pub shape: Option<ElementId>,
    pub connector: Option<ElementId>,
}

pub fn populate(app: &mut BlueprintApp, kind: DiagramKind) -> Targets {
    let targets = match kind {
        DiagramKind::Flowchart => flowchart(app),
        DiagramKind::Erd => erd(app),
        DiagramKind::Cloud => cloud(app),
    };
    app.selection.clear();
    app.fit_requested = false;
    app.status = "Screenshot fixture".into();
    app.doc.validate().expect("valid screenshot fixture");
    targets
}

fn rename_page(app: &mut BlueprintApp, name: &str) {
    assert!(app.apply(
        "Name example page",
        [Command::SetPage {
            id: app.page,
            prop: PageProp::Name(name.into()),
        }],
    ));
}

fn shape(app: &mut BlueprintApp, reference: &str, text: &str, bounds: Rect) -> ElementId {
    let id = app
        .insert_shape(
            ShapeRef::parse(reference).expect("fixture shape reference"),
            bounds,
        )
        .expect("insert fixture shape");
    set(app, id, [Prop::Text(text.into())]);
    id
}

fn set(app: &mut BlueprintApp, id: ElementId, props: impl IntoIterator<Item = Prop>) {
    assert!(app.apply(
        "Style example element",
        props.into_iter().map(|prop| Command::Set { id, prop }),
    ));
}

fn connection(
    app: &mut BlueprintApp,
    source: ElementId,
    source_port: &str,
    target: ElementId,
    target_port: &str,
    label: &str,
) -> ElementId {
    let id = app
        .insert_connector(
            Endpoint::glued(source, Some(source_port)),
            Endpoint::glued(target, Some(target_port)),
        )
        .expect("insert fixture connector");
    if !label.is_empty() {
        set(app, id, [Prop::Text(label.into())]);
    }
    id
}

fn flowchart(app: &mut BlueprintApp) -> Targets {
    rename_page(app, "Order processing");
    app.rename_layer(app.layer, "Process".into());
    let process_layer = app.layer;
    let reserve = shape(
        app,
        "flowchart/process",
        "Reserve inventory",
        Rect::new(240.0, 450.0, 450.0, 520.0),
    );
    let start = shape(
        app,
        "flowchart/terminator",
        "Start",
        Rect::new(255.0, 30.0, 435.0, 80.0),
    );
    let receive = shape(
        app,
        "flowchart/process",
        "Receive order",
        Rect::new(240.0, 130.0, 450.0, 190.0),
    );
    let approved = shape(
        app,
        "flowchart/decision",
        "Payment\napproved?",
        Rect::new(235.0, 245.0, 455.0, 355.0),
    );
    let ship = shape(
        app,
        "flowchart/process",
        "Ship order",
        Rect::new(240.0, 580.0, 450.0, 640.0),
    );
    let end = shape(
        app,
        "flowchart/terminator",
        "End",
        Rect::new(255.0, 695.0, 435.0, 745.0),
    );
    let notify = shape(
        app,
        "flowchart/process",
        "Notify customer",
        Rect::new(645.0, 270.0, 855.0, 335.0),
    );
    let cancelled = shape(
        app,
        "flowchart/terminator",
        "Order cancelled",
        Rect::new(655.0, 405.0, 845.0, 455.0),
    );
    for id in [reserve, receive, ship, notify] {
        set(
            app,
            id,
            [Prop::CornerRadius(Some(6.0)), Prop::FontSize(Some(15.0))],
        );
    }
    let primary_connector = connection(app, approved, "s", reserve, "n", "Yes");
    connection(app, start, "s", receive, "n", "");
    connection(app, receive, "s", approved, "n", "");
    connection(app, reserve, "s", ship, "n", "");
    connection(app, ship, "s", end, "n", "");
    connection(app, approved, "e", notify, "w", "No");
    connection(app, notify, "s", cancelled, "n", "");

    app.add_layer();
    app.rename_layer(app.layer, "Annotations".into());
    let note = shape(
        app,
        "basic/sticky-note",
        "Reserve stock\nbefore shipment.",
        Rect::new(640.0, 520.0, 845.0, 600.0),
    );
    let annotation = connection(app, reserve, "e", note, "w", "");
    set(
        app,
        annotation,
        [
            Prop::Routing(Routing::Straight),
            Prop::Dash(Some(Dash::Dashed)),
            Prop::EndMarker(Marker::None),
            Prop::Stroke(Some(Paint::Color(Color::rgb(202, 138, 4)))),
        ],
    );
    app.layer = process_layer;
    let page = app.page;
    app.add_page_with_kind(DiagramKind::Flowchart);
    rename_page(app, "Refunds");
    app.set_page(page);
    Targets {
        shape: Some(reserve),
        connector: Some(primary_connector),
    }
}

fn erd(app: &mut BlueprintApp) -> Targets {
    rename_page(app, "Shop schema");
    app.rename_layer(app.layer, "Tables".into());
    // SQL import gives each relationship an actual mapped FK definition,
    // accurate nullability/cardinality, and stable column identities.
    let sql = "CREATE TABLE public.orders (
        id uuid PRIMARY KEY,
        customer_id uuid NOT NULL REFERENCES public.customers(id) ON DELETE RESTRICT,
        placed_at timestamp NOT NULL,
        status varchar NOT NULL
    );
    CREATE TABLE public.customers (
        id uuid PRIMARY KEY,
        email varchar NOT NULL UNIQUE,
        name varchar
    );
    CREATE TABLE public.payments (
        id uuid PRIMARY KEY,
        order_id uuid NOT NULL REFERENCES public.orders(id),
        amount numeric NOT NULL
    );
    CREATE TABLE public.order_items (
        id uuid PRIMARY KEY,
        order_id uuid NOT NULL REFERENCES public.orders(id),
        product_id uuid NOT NULL REFERENCES public.products(id),
        quantity integer NOT NULL
    );
    CREATE TABLE public.products (
        id uuid PRIMARY KEY,
        name varchar NOT NULL,
        price numeric NOT NULL
    );
    CREATE INDEX orders_customer_idx ON public.orders(customer_id);";
    let preview = bp_sql::parse(sql, SqlDialect::PostgreSql).expect("valid fixture SQL");
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    // Use the actual import action so inserted tables AND connectors receive
    // the same authored dark-page presets as normal editor content.
    app.paste_sql_schema();
    app.sql.import.as_mut().expect("SQL import dialog").source = sql.into();
    app.preview_sql_import();
    assert!(app.apply_sql_import());
    let order: Vec<_> = app
        .doc
        .paint_order(app.page)
        .iter()
        .map(|element| (element.id, element.text().unwrap_or_default().to_owned()))
        .collect();
    let find = |name: &str| order.iter().find(|(_, text)| text == name).unwrap().0;
    let orders = find("orders");
    for (name, bounds) in [
        ("customers", Rect::new(30.0, 90.0, 310.0, 245.0)),
        ("orders", Rect::new(410.0, 80.0, 690.0, 280.0)),
        ("payments", Rect::new(790.0, 115.0, 1070.0, 270.0)),
        ("order_items", Rect::new(410.0, 435.0, 690.0, 635.0)),
        ("products", Rect::new(790.0, 455.0, 1070.0, 610.0)),
    ] {
        set(
            app,
            find(name),
            [Prop::Bounds(bounds), Prop::FontSize(Some(14.0))],
        );
    }
    let relationship = app
        .doc
        .paint_order(app.page)
        .into_iter()
        .find(|element| {
            element
                .as_connector()
                .is_some_and(|connector| connector.source.element() == Some(orders))
        })
        .map(|element| element.id);
    Targets {
        shape: Some(orders),
        connector: relationship,
    }
}

pub fn cloud_catalogs() -> Vec<IconPack> {
    [CloudProvider::Aws, CloudProvider::Azure]
        .into_iter()
        .map(|provider| IconPack {
            provider,
            version: "fixture-v1".into(),
            warnings: Vec::new(),
            icons: [
                ("load-balancer", "Load balancer", "Networking", "#8b5cf6", "M18 20h28M32 20v14M18 34h28M18 34v10M46 34v10"),
                ("container-service", "Container service", "Compute", "#ea8800", "M18 21l14-7 14 7v23l-14 7-14-7zM18 21l14 7 14-7M32 28v23"),
                ("postgresql", "PostgreSQL database", "Database", "#2563eb", "M18 22c0-9 28-9 28 0v24c0 9-28 9-28 0zM18 22c0 9 28 9 28 0M18 34c0 9 28 9 28 0"),
                ("object-storage", "Object storage", "Storage", "#43870c", "M17 19c0-7 30-7 30 0l-4 28c0 7-22 7-22 0zM17 19c0 7 30 7 30 0M25 35h14"),
            ]
            .into_iter()
            .map(|(slug, name, category, color, path)| CloudIcon {
                reference: ShapeRef::new(provider.id(), &format!("fixture-{slug}@fixture-v1")),
                name: name.into(),
                provider,
                category: category.into(),
                kind: IconKind::Service,
                pack_version: "fixture-v1".into(),
                source_path: format!("Original fixture artwork/{slug}.svg"),
                svg: format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\" viewBox=\"0 0 64 64\"><rect x=\"2\" y=\"2\" width=\"60\" height=\"60\" rx=\"10\" fill=\"{color}\"/><path d=\"{path}\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"2.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/></svg>"
                )
                .into(),
            })
            .collect(),
        })
        .collect()
}

fn cloud_boundary(app: &mut BlueprintApp, text: &str, bounds: Rect, color: Color) {
    let id = shape(app, "basic/rounded-rectangle", text, bounds);
    set(
        app,
        id,
        [
            Prop::Fill(Some(Paint::None)),
            Prop::Stroke(Some(Paint::Color(color))),
            Prop::StrokeWidth(Some(1.5)),
            Prop::Dash(Some(Dash::Dashed)),
            Prop::TextColor(Some(Color::rgb(0xee, 0xf2, 0xf6))),
            Prop::FontSize(Some(14.0)),
            Prop::TextAlign(Some(TextAlign::Left)),
            Prop::VerticalAlign(Some(VerticalAlign::Top)),
        ],
    );
}

fn cloud(app: &mut BlueprintApp) -> Targets {
    rename_page(app, "Retail cloud");
    assert!(app.apply(
        "Name cloud documentation layer",
        [Command::SetLayer {
            id: app.layer,
            prop: LayerProp::Name("Documentation".into()),
        }],
    ));
    cloud_boundary(
        app,
        "AWS region · us-east-1",
        Rect::new(20.0, 20.0, 1050.0, 695.0),
        Color::rgb(100, 116, 139),
    );
    cloud_boundary(
        app,
        "VPC · 10.0.0.0/16",
        Rect::new(40.0, 80.0, 760.0, 660.0),
        Color::rgb(139, 92, 246),
    );
    cloud_boundary(
        app,
        "Public subnet · 10.0.1.0/24",
        Rect::new(60.0, 140.0, 290.0, 620.0),
        Color::rgb(13, 148, 136),
    );
    cloud_boundary(
        app,
        "Private A · us-east-1a\n10.0.2.0/24",
        Rect::new(340.0, 140.0, 525.0, 415.0),
        Color::rgb(2, 132, 199),
    );
    cloud_boundary(
        app,
        "Private B · us-east-1b\n10.0.3.0/24",
        Rect::new(545.0, 140.0, 735.0, 415.0),
        Color::rgb(2, 132, 199),
    );
    // A subnet group references private subnets in two availability zones;
    // it is not model containment or a deployable fixture resource.
    cloud_boundary(
        app,
        "DB subnet group · Private A + B",
        Rect::new(340.0, 445.0, 735.0, 620.0),
        Color::rgb(100, 116, 139),
    );
    let api = shape(
        app,
        "aws/fixture-container-service@fixture-v1",
        "Orders API",
        Rect::new(420.0, 255.0, 496.0, 331.0),
    );
    let balancer = shape(
        app,
        "aws/fixture-load-balancer@fixture-v1",
        "Load balancer",
        Rect::new(135.0, 255.0, 211.0, 331.0),
    );
    let database = shape(
        app,
        "aws/fixture-postgresql@fixture-v1",
        "PostgreSQL · RDS",
        Rect::new(590.0, 490.0, 666.0, 566.0),
    );
    // Object storage is a regional service outside the VPC/subnet boundary.
    let storage = shape(
        app,
        "aws/fixture-object-storage@fixture-v1",
        "Order data",
        Rect::new(895.0, 255.0, 971.0, 331.0),
    );
    let primary_connector = connection(app, balancer, "e", api, "w", "HTTPS");
    connection(app, api, "s", database, "n", "SQL");
    connection(app, api, "e", storage, "w", "Object API");
    let note = shape(
        app,
        "basic/text",
        "Architecture documentation\nOriginal screenshot artwork\nNo deployment resources",
        Rect::new(800.0, 495.0, 1025.0, 590.0),
    );
    set(
        app,
        note,
        [
            Prop::FontSize(Some(13.0)),
            Prop::TextColor(Some(Color::rgb(0xb3, 0xbe, 0xcc))),
        ],
    );
    Targets {
        shape: Some(api),
        connector: Some(primary_connector),
    }
}
