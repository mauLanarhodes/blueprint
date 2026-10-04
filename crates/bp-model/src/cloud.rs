use crate::{ModelError, ShapeRef};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    Aws,
    Azure,
}

impl CloudProvider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Azure => "azure",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Aws => "AWS",
            Self::Azure => "Azure",
        }
    }

    pub fn source_url(self) -> &'static str {
        match self {
            Self::Aws => "https://aws.amazon.com/architecture/icons/",
            Self::Azure => "https://learn.microsoft.com/en-us/azure/architecture/icons/",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconKind {
    Service,
    Resource,
    Group,
}

/// An unmodified vendor SVG and its catalog metadata. Versioned references
/// are immutable, so old diagrams retain the exact icon they used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudIcon {
    pub reference: ShapeRef,
    pub name: String,
    pub provider: CloudProvider,
    pub category: String,
    pub kind: IconKind,
    pub pack_version: String,
    pub source_path: String,
    pub svg: Arc<str>,
}

impl CloudIcon {
    /// Checks metadata without bringing an SVG parser into the model.
    pub fn validate(&self) -> Result<(), ModelError> {
        let invalid = |message: &str| ModelError::InvalidIcon {
            reference: self.reference.clone(),
            message: message.into(),
        };
        if ShapeRef::parse(self.reference.as_str()).is_none()
            || self.reference.library() != self.provider.id()
        {
            return Err(invalid("the reference does not match its provider"));
        }
        let Some((id, version)) = self.reference.shape().split_once('@') else {
            return Err(invalid("the reference has no pack version"));
        };
        if id.is_empty()
            || matches!(id, "." | "..")
            || version != self.pack_version
            || version.is_empty()
            || matches!(version, "." | "..")
            || version
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        {
            return Err(invalid(
                "the reference has an invalid or mismatched pack version",
            ));
        }
        if self.name.trim().is_empty() || self.svg.trim().is_empty() {
            return Err(invalid("the icon needs a name and SVG data"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Element, OrderKey, Parent};
    use kurbo::Rect;

    fn icon() -> CloudIcon {
        CloudIcon {
            reference: ShapeRef::new("azure", "sample@v24"),
            name: "Sample".into(),
            provider: CloudProvider::Azure,
            category: "Compute".into(),
            kind: IconKind::Service,
            pack_version: "v24".into(),
            source_path: "Compute/Sample.svg".into(),
            svg: Arc::from("<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
        }
    }

    #[test]
    fn cloud_references_require_matching_document_assets() {
        let mut doc = Document::new();
        let parent = Parent::Layer(doc.layers_of(doc.first_page().unwrap())[0].id);
        let shape = Element::shape(
            icon().reference,
            parent,
            OrderKey::first(),
            Rect::new(0.0, 0.0, 64.0, 64.0),
        );
        doc.elements.insert(shape.id, shape);
        assert!(matches!(
            doc.validate(),
            Err(ModelError::MissingIcon { .. })
        ));
        doc.icons.insert(icon().reference.clone(), icon());
        assert_eq!(doc.validate(), Ok(()));
        let mut asset = icon();
        asset.provider = CloudProvider::Aws;
        doc.icons.insert(asset.reference.clone(), asset);
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidIcon { .. })
        ));
        let mut asset = icon();
        asset.pack_version = "v25".into();
        assert!(asset.validate().is_err());
        doc.icons.clear();
        doc.icons
            .insert(ShapeRef::new("azure", "other@v24"), icon());
        assert!(matches!(
            doc.validate(),
            Err(ModelError::InvalidIcon { .. })
        ));
    }
}
