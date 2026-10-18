use bp_model::AwsDeployment;
use std::path::PathBuf;

pub fn run(args: &[String], trial: bool) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut page = None;
    let mut with_lambda = false;
    let mut target = AwsDeployment::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lambda" if trial => with_lambda = true,
            flag @ ("--page" | "--account" | "--region" | "--profile") => {
                let value = args
                    .get(i + 1)
                    .filter(|s| !s.starts_with("--"))
                    .ok_or_else(|| format!("{flag} needs a value"))?
                    .clone();
                if trial {
                    match flag {
                        "--account" => target.account_id = value,
                        "--region" => target.region = value,
                        "--profile" => target.profile = value,
                        _ => {
                            return Err(
                                "aws-trial creates one page; --page is for aws-export".into()
                            );
                        }
                    }
                } else if flag == "--page" {
                    page = Some(value);
                } else {
                    return Err("Set the AWS target in the project before exporting".into());
                }
                i += 1;
            }
            flag if flag.starts_with('-') => return Err(format!("Unknown option {flag}")),
            path => positional.push(PathBuf::from(path)),
        }
        i += 1;
    }
    if trial {
        if positional.len() != 1 {
            return Err("aws-trial needs one output project path".into());
        }
        let output = &positional[0];
        if output.exists() {
            return Err(format!(
                "{} already exists; choose a new project path",
                output.display()
            ));
        }
        let doc = if with_lambda {
            bp_deploy::trial::document_with_lambda(target)
        } else {
            bp_deploy::trial::document(target)
        };
        if !doc.pages[&doc.first_page().unwrap()]
            .deployment
            .as_ref()
            .unwrap()
            .account_id
            .is_empty()
        {
            bp_deploy::configuration(&doc, doc.first_page().unwrap())?;
        }
        bp_io::save(&doc, output).map_err(|e| e.to_string())?;
        println!(
            "Created {}. Open it in Blueprint, set your AWS target, then preview deployment.",
            output.display()
        );
    } else {
        if positional.len() != 2 {
            return Err("aws-export needs an input project and output.tf.json path".into());
        }
        let input = &positional[0];
        let output = &positional[1];
        let doc = bp_io::load(input).map_err(|e| format!("{}: {e}", input.display()))?;
        if input.canonicalize().ok() == output.canonicalize().ok() {
            return Err("The Terraform output must be different from the input project".into());
        }
        if !output.to_string_lossy().ends_with(".tf.json") {
            return Err("Terraform JSON output must end in .tf.json".into());
        }
        let pages = doc.pages_sorted();
        let chosen = match page {
            None => *pages.first().ok_or("The project has no pages")?,
            Some(want) => want
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|i| pages.get(i))
                .copied()
                .or_else(|| pages.iter().find(|p| p.name == want).copied())
                .ok_or_else(|| format!("No page {want:?}"))?,
        };
        let config = bp_deploy::configuration(&doc, chosen.id)?;
        bp_deploy::write_configuration(&config, output, Some(input))?;
        println!(
            "Wrote {} ({} configured services, {} code packages). No AWS operations were performed.",
            output.display(),
            config.resources.len(),
            config.packages.len()
        );
    }
    Ok(())
}
