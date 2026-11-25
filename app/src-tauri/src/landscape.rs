use crate::systems::System;
use roxmltree::Node;
use std::{collections::HashMap, path::PathBuf};

pub fn default_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(r"SAP\Common\SAPUILandscape.xml"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Preferences/SAP/SAPGUILandscape.xml"))
    } else {
        // shortcut: Linux path not verified on a real install yet.
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".SAPGUI/SAPGUILandscape.xml"))
    }
}

pub fn parse(xml: &str) -> Result<Vec<System>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let folders = folders(&doc);
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name("Service") && n.attribute("type") == Some("SAPGUI"))
        // Only direct application-server entries: msid marks a message-server entry, routerid a SAProuter entry.
        .filter(|n| n.attribute("msid").is_none() && n.attribute("routerid").is_none())
        .filter_map(|n| {
            let (host, port) = n.attribute("server")?.rsplit_once(':')?;
            let sysnr = port
                .strip_prefix("32")
                .filter(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()))?;
            Some(System {
                id: n.attribute("uuid").unwrap_or(host).into(),
                name: n.attribute("name").unwrap_or(host).into(),
                sid: n.attribute("systemid").unwrap_or_default().to_uppercase(),
                host: host.into(),
                sysnr: sysnr.into(),
                folder: n.attribute("uuid").and_then(|u| folders.get(u)).cloned().unwrap_or_default(),
                ..Default::default()
            })
        })
        .collect())
}

/// Maps each service uuid to its folder path: Workspace (only if there are several) then Node names.
/// A service listed in more than one folder keeps the first one.
fn folders(doc: &roxmltree::Document) -> HashMap<String, Vec<String>> {
    fn walk(node: Node, path: &mut Vec<String>, out: &mut HashMap<String, Vec<String>>) {
        for child in node.children().filter(|c| c.is_element()) {
            if child.has_tag_name("Item") {
                if let Some(id) = child.attribute("serviceid") {
                    out.entry(id.into()).or_insert_with(|| path.clone());
                }
            } else if child.has_tag_name("Node") {
                path.push(child.attribute("name").unwrap_or_default().into());
                walk(child, path, out);
                path.pop();
            }
        }
    }
    let workspaces: Vec<_> = doc.descendants().filter(|n| n.has_tag_name("Workspace")).collect();
    let mut out = HashMap::new();
    for ws in &workspaces {
        let mut path = Vec::new();
        if workspaces.len() > 1 {
            path.push(ws.attribute("name").unwrap_or_default().into());
        }
        walk(*ws, &mut path, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_direct_entries_and_skips_others() {
        let xml = r#"<Landscape><Services>
            <Service type="SAPGUI" uuid="u1" name="DEV" systemid="DEV" server="dev.example:3200"/>
            <Service type="SAPGUI" uuid="u2" name="LB" msid="m1" server="PUBLIC"/>
            <Service type="SAPGUI" uuid="u3" name="RT" routerid="r1" server="h:3201"/>
            <Service type="SAPGUI" uuid="u4" name="Odd" server="h:8000"/>
            <Service type="NWBC" uuid="u5" name="Web" server="h:3200"/>
        </Services></Landscape>"#;
        let systems = parse(xml).unwrap();
        assert_eq!(systems.len(), 1);
        let s = &systems[0];
        assert_eq!((s.id.as_str(), s.name.as_str(), s.sid.as_str()), ("u1", "DEV", "DEV"));
        assert_eq!((s.host.as_str(), s.sysnr.as_str()), ("dev.example", "00"));
    }

    #[test]
    fn reads_folders_from_workspaces_and_nodes() {
        let service = |id: &str| format!(r#"<Service type="SAPGUI" uuid="{id}" name="{id}" server="h:3200"/>"#);
        let xml = format!(
            r#"<Landscape><Services>{}{}{}</Services><Workspaces>
            <Workspace name="W1"><Node name="A"><Node name="B"><Item serviceid="u1"/></Node></Node><Item serviceid="u2"/></Workspace>
            <Workspace name="W2"><Node name="C"><Item serviceid="u1"/></Node></Workspace>
            </Workspaces></Landscape>"#,
            service("u1"), service("u2"), service("u3"),
        );
        let systems = parse(&xml).unwrap();
        let folder = |id: &str| systems.iter().find(|s| s.id == id).unwrap().folder.join("/");
        assert_eq!(folder("u1"), "W1/A/B");
        assert_eq!(folder("u2"), "W1");
        assert_eq!(folder("u3"), "");
    }

    #[test]
    fn invalid_xml_is_an_error() {
        assert!(parse("<Landscape>").is_err());
    }
}
