"""Rebuild the bundled inventory from the supplied provider CSV, or check drift."""
import argparse
import csv
import hashlib
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "crates/bp-deploy/catalogs"
    source = root / "resources-2026-10-14.csv"
    with source.open(newline="", encoding="utf-8") as stream:
        reader = csv.DictReader(stream)
        if reader.fieldnames != ["provider", "provider_version", "service_group", "terraform_resource_type"]:
            raise SystemExit("Unexpected CSV columns")
        rows = list(reader)
    seen = set()
    for row in rows:
        identity = (row["provider"], row["terraform_resource_type"])
        if identity in seen or row["provider"] not in ("aws", "awscc") or not all(row.values()):
            raise SystemExit(f"Invalid or duplicate inventory entry: {identity}")
        seen.add(identity)
    providers = []
    for name in ("aws", "awscc"):
        entries = [row for row in rows if row["provider"] == name]
        versions = {row["provider_version"] for row in entries}
        if len(versions) != 1:
            raise SystemExit(f"{name} must have exactly one provider release")
        version = versions.pop()
        providers.append({"provider": name, "version": version, "resource_count": len(entries),
                          "group_count": len({row["service_group"] for row in entries}),
                          "source_url": f"https://registry.terraform.io/v1/providers/hashicorp/{name}/{version}"})
    manifest = {"format_version": 1, "snapshot": "2026-10-14",
                "provenance": "User-supplied provider catalog; schemas and deployment testing tracked separately",
                "csv_sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "providers": providers}
    outputs = {"manifest.json": json.dumps(manifest, indent=2) + "\n",
               "resources.json": "[\n" + ",\n".join("  " + json.dumps(row, separators=(",", ":")) for row in rows) + "\n]\n"}
    for name, content in outputs.items():
        path = root / name
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                raise SystemExit(f"Catalog drift: run python tools/update_aws_catalog.py ({name})")
        else:
            path.write_text(content, encoding="utf-8")
    print(f"{'Checked' if args.check else 'Generated'} {len(rows)} provider catalog entries")


if __name__ == "__main__":
    main()
