use crate::IconPack;
use bp_model::CloudIcon;

/// Searchable familiar names in addition to the provider's official title.
pub fn aliases(icon: &CloudIcon) -> &'static [&'static str] {
    let name = icon.name.to_ascii_lowercase();
    if name.contains("ec2") {
        &["compute", "virtual machine", "vm", "instance"]
    } else if name.contains("s3") || name.contains("simple storage service") {
        &["bucket", "object storage", "storage"]
    } else if name.contains("lambda") {
        &["serverless", "function", "compute"]
    } else if name.contains("blob") {
        &["bucket", "object storage", "storage"]
    } else if name.contains("virtual machine") {
        &["vm", "virtualmachine", "compute", "instance"]
    } else if name.contains("function") {
        &["serverless", "lambda", "compute"]
    } else {
        &[]
    }
}

/// Case-insensitive search of provider, category, official name and aliases.
/// Every query word must match, so `compute vm` usefully narrows large packs.
pub fn search<'a>(pack: &'a IconPack, query: &str) -> Vec<&'a CloudIcon> {
    let words: Vec<_> = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    pack.icons
        .iter()
        .filter(|icon| {
            let haystack = format!(
                "{} {} {} {}",
                icon.provider.label(),
                icon.category,
                icon.name,
                aliases(icon).join(" ")
            )
            .to_ascii_lowercase();
            words.iter().all(|word| haystack.contains(word))
        })
        .collect()
}
