use std::{
    collections::HashSet,
    path::{Component, Path},
};

use ariadeck_ui::AddDownloadMetadataKindView;
use data_encoding::HEXLOWER;
use lava_torrent::torrent::v1::Torrent;
use quick_xml::{Reader, XmlVersion, events::Event};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataPreview {
    pub content_sha256: String,
    pub info_hash: Option<String>,
    pub files: Vec<MetadataPreviewFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataPreviewFile {
    pub index: u32,
    pub path: String,
    pub length: Option<u64>,
}

pub fn parse_metadata(
    kind: AddDownloadMetadataKindView,
    content: &[u8],
) -> Result<MetadataPreview, String> {
    let (files, info_hash) = match kind {
        AddDownloadMetadataKindView::Torrent => {
            let (files, info_hash) = parse_torrent(content)?;
            (files, Some(info_hash))
        }
        AddDownloadMetadataKindView::Metalink => (parse_metalink(content)?, None),
    };
    if files.is_empty() {
        return Err("Metadata does not contain any downloadable files.".into());
    }
    Ok(MetadataPreview {
        content_sha256: HEXLOWER.encode(&Sha256::digest(content)),
        info_hash,
        files,
    })
}

fn parse_torrent(content: &[u8]) -> Result<(Vec<MetadataPreviewFile>, String), String> {
    let torrent = Torrent::read_from_bytes(content)
        .map_err(|error| format!("Invalid Torrent metadata: {error}"))?;
    let info_hash = torrent.info_hash();
    let root = normalize_relative_path(Path::new(&torrent.name))?;
    let files = match torrent.files {
        Some(files) => files
            .into_iter()
            .enumerate()
            .map(|(offset, file)| {
                let relative = normalize_relative_path(&file.path)?;
                let length = u64::try_from(file.length)
                    .map_err(|_| format!("Torrent file has a negative length: {relative}"))?;
                preview_file(offset, format!("{root}/{relative}"), Some(length))
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(validate_unique_paths)?,
        None => {
            let length = u64::try_from(torrent.length)
                .map_err(|_| format!("Torrent file has a negative length: {root}"))?;
            vec![MetadataPreviewFile {
                index: 1,
                path: root,
                length: Some(length),
            }]
        }
    };
    Ok((files, info_hash))
}

fn parse_metalink(content: &[u8]) -> Result<Vec<MetadataPreviewFile>, String> {
    let mut reader = Reader::from_reader(content);
    reader.config_mut().trim_text(true);
    let mut files = Vec::new();
    let mut current: Option<MetalinkFile> = None;
    let mut reading_size = false;

    loop {
        match reader
            .read_event()
            .map_err(|error| format!("Invalid Metalink XML: {error}"))?
        {
            Event::Start(element) if element.local_name().as_ref() == b"file" => {
                if current.is_some() {
                    return Err(
                        "Invalid Metalink XML: nested file elements are unsupported.".into(),
                    );
                }
                current = Some(MetalinkFile {
                    path: metalink_file_name(&reader, &element)?,
                    size: None,
                    size_text: String::new(),
                });
            }
            Event::Empty(element) if element.local_name().as_ref() == b"file" => {
                let path = metalink_file_name(&reader, &element)?;
                files.push(preview_file(files.len(), path, None)?);
            }
            Event::Start(element)
                if current.is_some() && element.local_name().as_ref() == b"size" =>
            {
                reading_size = true;
            }
            Event::Text(text) if reading_size => {
                let value = text
                    .decode()
                    .map_err(|error| format!("Invalid Metalink size text: {error}"))?;
                if let Some(file) = &mut current {
                    file.size_text.push_str(&value);
                }
            }
            Event::CData(text) if reading_size => {
                let value = text
                    .decode()
                    .map_err(|error| format!("Invalid Metalink size text: {error}"))?;
                if let Some(file) = &mut current {
                    file.size_text.push_str(&value);
                }
            }
            Event::End(element) if element.local_name().as_ref() == b"size" => {
                reading_size = false;
                if let Some(file) = &mut current {
                    let value = file.size_text.trim();
                    if !value.is_empty() {
                        file.size = Some(value.parse::<u64>().map_err(|error| {
                            format!("Invalid Metalink file size {value:?}: {error}")
                        })?);
                    }
                }
            }
            Event::End(element) if element.local_name().as_ref() == b"file" => {
                let file = current.take().ok_or_else(|| {
                    "Invalid Metalink XML: file closing tag has no opening tag.".to_owned()
                })?;
                files.push(preview_file(files.len(), file.path, file.size)?);
                reading_size = false;
            }
            Event::DocType(_) => {
                return Err("Metalink documents with a DTD are not accepted.".into());
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if current.is_some() {
        return Err("Invalid Metalink XML: unterminated file element.".into());
    }
    validate_unique_paths(files)
}

struct MetalinkFile {
    path: String,
    size: Option<u64>,
    size_text: String,
}

fn metalink_file_name(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<String, String> {
    for attribute in element.attributes() {
        let attribute =
            attribute.map_err(|error| format!("Invalid Metalink attribute: {error}"))?;
        if attribute.key.local_name().as_ref() == b"name" {
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|error| format!("Invalid Metalink file name: {error}"))?;
            return normalize_relative_path(Path::new(value.as_ref()));
        }
    }
    Err("Invalid Metalink metadata: file element is missing its name.".into())
}

fn preview_file(
    offset: usize,
    path: String,
    length: Option<u64>,
) -> Result<MetadataPreviewFile, String> {
    let index = u32::try_from(offset)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| "Metadata contains too many files to index.".to_owned())?;
    Ok(MetadataPreviewFile {
        index,
        path,
        length,
    })
}

fn validate_unique_paths(
    files: Vec<MetadataPreviewFile>,
) -> Result<Vec<MetadataPreviewFile>, String> {
    let mut paths = HashSet::with_capacity(files.len());
    for file in &files {
        if !paths.insert(file.path.clone()) {
            return Err(format!(
                "Metadata contains the same file path more than once: {}",
                file.path
            ));
        }
    }
    Ok(files)
}

fn normalize_relative_path(path: &Path) -> Result<String, String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(format!(
            "Metadata file path must be relative and non-empty: {}",
            path.display()
        ));
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => {
                parts.push(part.to_string_lossy().into_owned());
            }
            _ => {
                return Err(format!(
                    "Metadata file path contains an unsafe component: {}",
                    path.display()
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err("Metadata file path must not be empty.".into());
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torrent_preview_supports_single_and_multi_file_metadata() {
        let single_content = torrent_fixture(None, "fixture.bin", 1);
        let single = parse_metadata(AddDownloadMetadataKindView::Torrent, &single_content)
            .expect("single-file Torrent parses");
        assert_eq!(single.files[0].path, "fixture.bin");
        assert_eq!(single.files[0].length, Some(1));
        assert_eq!(single.content_sha256.len(), 64);
        assert_eq!(single.info_hash.as_deref().map(str::len), Some(40));

        let multi_content = torrent_fixture(
            Some(vec![
                lava_torrent::torrent::v1::File {
                    length: 3,
                    path: "a/bin".into(),
                    extra_fields: None,
                },
                lava_torrent::torrent::v1::File {
                    length: 4,
                    path: "b/bin".into(),
                    extra_fields: None,
                },
            ]),
            "root",
            7,
        );
        let multi = parse_metadata(AddDownloadMetadataKindView::Torrent, &multi_content)
            .expect("multi-file Torrent parses");
        assert_eq!(
            multi.files,
            vec![
                MetadataPreviewFile {
                    index: 1,
                    path: "root/a/bin".into(),
                    length: Some(3),
                },
                MetadataPreviewFile {
                    index: 2,
                    path: "root/b/bin".into(),
                    length: Some(4),
                },
            ]
        );
        assert_eq!(multi.info_hash.as_deref().map(str::len), Some(40));
    }

    #[test]
    fn metalink_preview_supports_v3_v4_and_unknown_sizes() {
        let content = br#"<?xml version="1.0"?>
            <metalink xmlns="urn:ietf:params:xml:ns:metalink">
              <file name="one.bin"><size>12</size></file>
              <file name="nested/two.bin" />
            </metalink>"#;
        let preview = parse_metadata(AddDownloadMetadataKindView::Metalink, content)
            .expect("Metalink parses");
        assert_eq!(preview.files.len(), 2);
        assert_eq!(preview.files[0].length, Some(12));
        assert_eq!(preview.files[1].path, "nested/two.bin");
        assert_eq!(preview.files[1].length, None);
        assert_eq!(preview.info_hash, None);
    }

    #[test]
    fn metadata_preview_rejects_unsafe_paths_doctypes_and_empty_lists() {
        for content in [
            br#"<metalink><file name="../escape.bin" /></metalink>"#.as_slice(),
            br#"<!DOCTYPE metalink><metalink><file name="safe.bin" /></metalink>"#.as_slice(),
            br#"<metalink />"#.as_slice(),
        ] {
            assert!(
                parse_metadata(AddDownloadMetadataKindView::Metalink, content).is_err(),
                "unsafe or empty Metalink must be rejected"
            );
        }
    }

    fn torrent_fixture(
        files: Option<Vec<lava_torrent::torrent::v1::File>>,
        name: &str,
        length: i64,
    ) -> Vec<u8> {
        Torrent {
            announce: None,
            announce_list: None,
            length,
            files,
            name: name.into(),
            piece_length: 16_384,
            pieces: vec![vec![0xff; 20]],
            extra_fields: None,
            extra_info_fields: None,
        }
        .encode()
        .expect("Torrent fixture encodes")
    }
}
